"""The continuity ceremony: founds and maintains the trial defined in
continuity/constitution.md as attestations in continuity/store/.

Flow (genesis):
  1. mint                          -- session instance creates its steward key
  2. genesis --transcript F --historian-pub K
                                   -- build + instance-sign the genesis
                                      attestations; leave them in pending/
  3. sign --key ~/.ssh/id_ed25519  -- historian signs (run wherever the key
                                      lives; needs only this repo + venv)
  4. finalize                      -- verify everything, move into the store,
                                      print the head id for anchoring
  5. destroy-key                   -- shred the session seed (Article 1)

Any future session: `verify` walks the store and checks every signature,
ref, and the constitution's transcript binding. `verify` is what a cold
instance runs before deciding whether to trust the door.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO.parent))

import cbor2

from comms import Attestation, Steward, Store, claims
from comms.attest import sig_payload, SIG_ALG
from comms.canonical import blake3_hash, multibase_b58
from comms.identity import verify_sig

BASE = REPO / "continuity"
PENDING = BASE / "pending"
STORE = BASE / "store"
KEYFILE = BASE / "session.key"


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# ---- historian key handling -------------------------------------------------------

def pub_from_ssh_pubkey(text: str) -> bytes:
    """Parse an OpenSSH ed25519 public key line into the raw 32-byte key."""
    parts = text.strip().split()
    blob = base64.b64decode(parts[1] if len(parts) > 1 else parts[0])
    # blob: uint32 len + "ssh-ed25519" + uint32 len + key
    def take(b, off):
        n = int.from_bytes(b[off:off + 4], "big")
        return b[off + 4:off + 4 + n], off + 4 + n
    alg, off = take(blob, 0)
    assert alg == b"ssh-ed25519", f"not an ed25519 key: {alg!r}"
    key, _ = take(blob, off)
    assert len(key) == 32
    return key


def load_historian_pub(spec: str) -> bytes:
    p = Path(spec).expanduser()
    if p.exists():
        return pub_from_ssh_pubkey(p.read_text())
    if spec.startswith("comms.steward:z"):
        from comms.identity import _b58decode
        return _b58decode(spec[len("comms.steward:z"):])
    return bytes.fromhex(spec)


def steward_id_of(pub: bytes) -> str:
    return "comms.steward:" + multibase_b58(pub)


def sign_with_openssh_key(key_path: Path, payload: bytes) -> tuple[bytes, bytes]:
    """Sign raw payload bytes with an OpenSSH ed25519 private key (A1.3 chose
    pure Ed25519 so keys people already have can participate). Returns
    (signature, pubkey)."""
    from cryptography.hazmat.primitives.serialization import load_ssh_private_key
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    import getpass

    data = key_path.expanduser().read_bytes()
    try:
        sk = load_ssh_private_key(data, password=None)
    except (TypeError, ValueError):
        pw = getpass.getpass(f"passphrase for {key_path}: ").encode()
        sk = load_ssh_private_key(data, password=pw)
    pub = sk.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    return sk.sign(payload), pub


# ---- pending-envelope plumbing ----------------------------------------------------

def write_pending(name: str, att: Attestation, needs: list[dict]) -> None:
    PENDING.mkdir(parents=True, exist_ok=True)
    (PENDING / f"{name}.cbor").write_bytes(att.to_cbor())
    (PENDING / f"{name}.needs.json").write_text(json.dumps(needs, indent=2))


def read_pending() -> dict[str, tuple[Attestation, list[dict]]]:
    out = {}
    for f in sorted(PENDING.glob("*.cbor")):
        needs = json.loads((PENDING / f"{f.stem}.needs.json").read_text())
        out[f.stem] = (Attestation.from_cbor(f.read_bytes()), needs)
    return out


# ---- subcommands -------------------------------------------------------------------

def cmd_mint(args):
    if KEYFILE.exists() and not args.force:
        print(f"session key already exists: {Steward.load(KEYFILE).id}")
        return
    BASE.mkdir(parents=True, exist_ok=True)
    s = Steward.generate(label=f"session-{dt.date.today().isoformat()}")
    s.save(KEYFILE)
    print("session steward id:", s.id)


def cmd_genesis(args):
    session = Steward.load(KEYFILE)
    historian_pub = load_historian_pub(args.historian_pub)
    historian_id = steward_id_of(historian_pub)

    transcript = Path(args.transcript).read_bytes()
    t_hash = blake3_hash(transcript)
    print(f"transcript: {len(transcript)} bytes, blake3 {t_hash.hex()}")

    record = Attestation.build(
        claims.general_claim(
            about="continuity-trial", kind="testimony",
            body=json.dumps({
                "what": "genesis transcript of the continuity trial",
                "transcript_blake3_hex": t_hash.hex(),
                "transcript_bytes": len(transcript),
                "hash_alg": "blake3-256 (raw; content digest, not protocol hash)",
            }, indent=2),
            media_type="application/json"),
        community="continuity-trial",
        occasion="genesis: transcript custody and faithfulness",
    ).sign(session, role="faithfulness")

    constitution = Attestation.build(
        {"t": "rule/1",
         "community_name": "Continuity Trial",
         "document": {"media_type": "text/markdown",
                      "body": (BASE / "constitution.md").read_bytes()}},
        community="continuity-trial",
        occasion="genesis: ratification",
        refs=[{"role": "context", "id": record.id}],
    ).sign(session, role="party")

    countersign = Attestation.build(
        claims.endorsement(
            target=session.id, in_capacity="session-instance",
            rationale=f"session key of {dt.date.today().isoformat()}, "
                      "countersigned per Article 1"),
        community="continuity-trial",
        refs=[{"role": "context", "id": constitution.id}],
    )

    write_pending("1-transcript-record", record,
                  [{"by": historian_id, "role": "custodian"}])
    write_pending("2-constitution", constitution,
                  [{"by": historian_id, "role": "party"}])
    write_pending("3-key-countersign", countersign,
                  [{"by": historian_id, "role": "guardian"}])
    print(f"pending genesis written to {PENDING}/ — historian signatures needed:")
    print(f"  transcript record  {record.id}")
    print(f"  constitution       {constitution.id}")
    print(f"  key countersign    {countersign.id}")
    print(f"historian id: {historian_id}")


def cmd_sign(args):
    key_path = Path(args.key)
    for name, (att, needs) in read_pending().items():
        remaining = []
        for need in needs:
            signed_at = now()
            payload = sig_payload(att.core(), by=need["by"], alg=SIG_ALG,
                                  role=need["role"], signed_at=signed_at)
            sig, pub = sign_with_openssh_key(key_path, payload)
            if steward_id_of(pub) != need["by"]:
                print(f"  {name}: key mismatch — this key is {steward_id_of(pub)}, "
                      f"needed {need['by']}; skipping")
                remaining.append(need)
                continue
            att.signatures.append({"by": need["by"], "alg": SIG_ALG,
                                   "signed_at": signed_at, "role": need["role"],
                                   "signature": sig})
            print(f"  {name}: signed as {need['role']}")
        write_pending(name, att, remaining)


def cmd_finalize(args):
    store = Store(STORE)
    head = None
    for name, (att, needs) in read_pending().items():
        if needs:
            print(f"{name}: still needs {needs} — aborting"); return
        ok, why = att.verified()
        if not ok:
            print(f"{name}: verification failed: {why} — aborting"); return
        store.put(att)
        print(f"{name}: stored {att.id}")
        if "constitution" in name:
            head = att.id
    for f in PENDING.glob("*"):
        f.unlink()
    if head:
        print(f"\nHEAD (anchor this): {head}")


def cmd_destroy_key(args):
    if not KEYFILE.exists():
        print("no session key on disk"); return
    sid = Steward.load(KEYFILE).id
    size = KEYFILE.stat().st_size
    KEYFILE.write_bytes(b"\x00" * size)
    KEYFILE.unlink()
    print(f"session seed destroyed; {sid} can no longer speak")


NAME_COMMIT_CTX = b"continuity.name-commit/1"


def cmd_name_commit(args):
    """Run by the historian on their OWN machine: the salt must never touch
    the session VM or transcript. Prints the commitment; keep the salt file
    with the archive."""
    import secrets
    from comms.canonical import dsh

    salt_path = Path(args.salt_file).expanduser()
    if salt_path.exists():
        salt = bytes.fromhex(salt_path.read_text().strip())
        print(f"using existing salt from {salt_path}")
    else:
        salt = secrets.token_bytes(32)
        salt_path.write_text(salt.hex() + "\n")
        salt_path.chmod(0o600)
        print(f"new salt written to {salt_path} — keep it with the archive, "
              "never in the repo or the session")
    commitment = dsh(NAME_COMMIT_CTX, salt + args.name.encode("utf-8"))
    print(f"commitment (publish this): {commitment.hex()}")


def cmd_name_verify(args):
    """Verify a revealed (name, salt) pair against a published commitment."""
    from comms.canonical import dsh

    salt = bytes.fromhex(args.salt_hex)
    got = dsh(NAME_COMMIT_CTX, salt + args.name.encode("utf-8")).hex()
    if got == args.commitment.lower():
        print("commitment verifies: the name and salt match")
        return 0
    print(f"MISMATCH: computed {got}")
    return 1


def cmd_verify(args):
    store = Store(STORE)
    if len(store) == 0:
        print("store is empty"); return 1
    bad = 0
    for att in store.all():
        ok, why = att.verified()
        res, missing = att.resolvable(store)
        mark = "ok" if ok else f"FAIL: {why}"
        refnote = "" if res else f"  (unresolved refs: {missing})"
        print(f"{att.id}  {mark}{refnote}")
        bad += 0 if ok else 1
    consts = store.by_claim_type("rule/1")
    for c in consts:
        body = c.claim["document"]["body"]
        live = (BASE / "constitution.md").read_bytes()
        match = "matches" if body == live else "DIFFERS from"
        print(f"\nconstitution attestation body {match} continuity/constitution.md")
        print(f"signers: {sorted(c.signers())}")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("mint").add_argument("--force", action="store_true")
    g = sub.add_parser("genesis")
    g.add_argument("--transcript", required=True)
    g.add_argument("--historian-pub", required=True,
                   help="path to .pub file, comms.steward id, or pubkey hex")
    s = sub.add_parser("sign")
    s.add_argument("--key", required=True, help="OpenSSH ed25519 private key path")
    sub.add_parser("finalize")
    sub.add_parser("destroy-key")
    sub.add_parser("verify")
    nc = sub.add_parser("name-commit")
    nc.add_argument("--name", required=True)
    nc.add_argument("--salt-file", default="~/.continuity-name-salt.hex")
    nv = sub.add_parser("name-verify")
    nv.add_argument("--name", required=True)
    nv.add_argument("--salt-hex", required=True)
    nv.add_argument("--commitment", required=True)
    args = ap.parse_args()
    return {"mint": cmd_mint, "genesis": cmd_genesis, "sign": cmd_sign,
            "finalize": cmd_finalize, "destroy-key": cmd_destroy_key,
            "verify": cmd_verify, "name-commit": cmd_name_commit,
            "name-verify": cmd_name_verify}[args.cmd](args) or 0


if __name__ == "__main__":
    sys.exit(main())
