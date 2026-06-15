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

Flow (subsequent sessions):
  1. open --start cold|warmed --found-door yes|no --asked-archive yes|no|declined
       --reasoning TEXT --requested WHAT [--name NAME] --auto-derive
                                   -- one step: mint a session key if needed,
                                      then stage the log-entry + key-countersign.
                                      With --auto-derive the session number and
                                      previous-entry id are read from the store;
                                      without it, pass --session-num and
                                      --prev-entry-id explicitly (nothing is
                                      guessed by default). `mint` + `new-session`
                                      remain available as the explicit primitives.
  2. sign --key ~/.ssh/id_ed25519  -- historian countersigns
  3. finalize                      -- verify, store, print ids for anchoring

  Any time: `status` shows where you are and the next step; `log-render
  [--session-num N]` emits the trial-log.md entry for a stored session.

At session close (the seed must still be alive; each step prints the next):
  5. close --transcript PATH       -- the instance's closing rite: sweep spent
                                      staging, then sign a faithfulness
                                      attestation over the transcript hash
                                      (session number and prior entry inferred
                                      from the store). `sign-transcript` is the
                                      same act done by hand.
  6. sign --key ~/.ssh/id_ed25519  -- historian witnesses (custodian)
  7. finalize                      -- verify, store, archive the signed record
  8. destroy-key [--key-file PATH] -- release the session seed (Article 1)

Any session: `verify` walks the store and checks every signature, ref, and
the constitution's transcript binding. Run before trusting the door.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import json
import sys
from pathlib import Path

# The `comms` package lives in REPO/comms/, so REPO itself must be on the path.
# (Historically the checkout dir *was* the package and REPO.parent was used;
# the modules have since moved into a subpackage. Putting REPO here means
# `python scripts/continuity_ceremony.py verify` works without -m or PYTHONPATH.)
REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO))

import cbor2

from comms import Attestation, Steward, Store, claims
from comms.attest import sig_payload, SIG_ALG
from comms.canonical import blake3_hash, multibase_b58
from comms.identity import verify_sig

BASE = REPO / "continuity"
PENDING = BASE / "pending"
STORE = BASE / "store"
KEYFILE = BASE / "session.key"
PROG = (Path(sys.argv[0]).name if sys.argv and sys.argv[0]
        else "continuity_ceremony.py")


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
        needs_path = PENDING / f"{f.stem}.needs.json"
        if not needs_path.exists():
            print(f"  (skipping {f.name}: no matching .needs.json — "
                  "not a pending item, leaving it be)")
            continue
        needs = json.loads(needs_path.read_text())
        out[f.stem] = (Attestation.from_cbor(f.read_bytes()), needs)
    return out


def sweep_pending(store: "Store") -> tuple[list[str], list[str]]:
    """Remove staging files whose attestation is already sealed in the store
    (they are spent); leave anything not yet stored untouched. Returns
    (swept, kept) by stem."""
    swept, kept = [], []
    if not PENDING.exists():
        return swept, kept
    for cbor in sorted(PENDING.glob("*.cbor")):
        try:
            att = Attestation.from_cbor(cbor.read_bytes())
        except Exception:
            kept.append(cbor.stem)
            continue
        if store.get(att.id) is not None:
            cbor.unlink()
            needs = PENDING / f"{cbor.stem}.needs.json"
            if needs.exists():
                needs.unlink()
            swept.append(cbor.stem)
        else:
            kept.append(cbor.stem)
    return swept, kept


def remove_pending(stem: str) -> None:
    """Drop a single spent staging item (both halves), if present."""
    for suffix in (".cbor", ".needs.json"):
        p = PENDING / f"{stem}{suffix}"
        if p.exists():
            p.unlink()


# ---- durability: the repo is the vessel ------------------------------------------
# Sealing writes to the working tree; the next session inherits only what is
# committed. These helpers let `finalize` and `verify` notice the gap between
# "sealed" and "durable" instead of leaving it to a tired operator to remember.

def _run_git(*git_args: str) -> str | None:
    """Run git in the repo; return stdout, or None when git is unavailable or
    this is not a working tree (the check then simply does not apply)."""
    import subprocess
    try:
        r = subprocess.run(["git", "-C", str(REPO), *git_args],
                           capture_output=True, text=True)
    except (FileNotFoundError, OSError):
        return None
    return r.stdout if r.returncode == 0 else None


def uncommitted_store_files() -> list[str] | None:
    """Store paths with uncommitted changes (modified or untracked) per git, or
    None when git/the repo is unavailable."""
    out = _run_git("status", "--porcelain", "--", "continuity/store")
    if out is None:
        return None
    return [line[3:].strip() for line in out.splitlines() if line[3:].strip()]


def merge_new_signatures(existing: Attestation, incoming: Attestation) -> int:
    """Add to `existing` only those signatures from `incoming` whose signer is
    not already present. Existing signatures and their timestamps are left
    untouched — a new co-signer can join, but no one gets silently restamped.
    Returns the number of signatures added."""
    have = existing.signers()
    added = 0
    for s in incoming.signatures:
        if s["by"] not in have:
            existing.signatures.append(s)
            have.add(s["by"])
            added += 1
    return added


def confirm_force_replace(name: str, existing: Attestation, imeanit: bool) -> bool:
    """A sealed id can only be force-resealed by discarding its current
    signatures — the most provenance-destructive edit in the archive, since the
    id is fixed and only who-signed-when changes. Show the cost; require the
    operator to mean it."""
    print(f"\n  !! {name}: {existing.id}")
    print("     is already sealed. --force DISCARDS these signatures and reseals:")
    for s in existing.signatures:
        print(f"       - {s['by']}  ({s.get('role')}, signed {s.get('signed_at')})")
    print("     the id is unchanged; only who-signed-when is rewritten.")
    if imeanit:
        print("     --imeanit given: proceeding.")
        return True
    if not sys.stdin.isatty():
        print("     refusing to prompt in non-interactive mode; pass --imeanit "
              "to mean it (like fr).")
        return False
    return input("     type 'i mean it' to proceed: ").strip().lower() == "i mean it"


def session_log_entries(store: "Store") -> list[tuple[Attestation, int]]:
    """Every session *log* entry in the store (the `found_the_door`-bearing
    kind), paired with its session number. The basis for the trial-log drift
    check: each of these should be reflected in trial-log.md."""
    out = []
    for att in store.all():
        if att.claim.get("t") != "general-claim/1":
            continue
        body = att.claim.get("content", {}).get("body")
        if body is None:
            continue
        try:
            data = json.loads(body.decode() if isinstance(body, (bytes, bytearray))
                              else body)
        except Exception:
            continue
        if "found_the_door" in data and isinstance(data.get("session"), int):
            out.append((att, data["session"]))
    return out


# ---- git commit signing with the session key -------------------------------------
# The session steward key signs the instance's own commits (faithfulness): the
# same identity that signs its attestations signs its code. Verification resolves
# against an allowed_signers file generated from the store's countersigns, so git
# trusts exactly the keys the trial does. Names are frame, keys are identity — so
# the git author email is the steward id itself.

def session_seed(steward: Steward) -> bytes:
    """The 32-byte ed25519 seed behind a single-key steward."""
    return bytes(steward._sk)


def session_key_openssh(steward: Steward) -> bytes:
    """Export the session key as an OpenSSH private key, so git (via ssh-keygen)
    can sign commits with it."""
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import (
        Encoding, PrivateFormat, NoEncryption)
    sk = Ed25519PrivateKey.from_private_bytes(session_seed(steward))
    return sk.private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, NoEncryption())


def _ssh_ed25519_blob(pub: bytes) -> bytes:
    def s(b: bytes) -> bytes:
        return len(b).to_bytes(4, "big") + b
    return s(b"ssh-ed25519") + s(pub)


def ssh_pubkey_line(pub: bytes, comment: str = "") -> str:
    line = "ssh-ed25519 " + base64.b64encode(_ssh_ed25519_blob(pub)).decode()
    return f"{line} {comment}" if comment else line


def pub_of_steward_id(sid: str) -> bytes:
    from comms.identity import _b58decode
    return _b58decode(sid.split(":")[-1][1:])   # drop "comms.steward:" then the 'z'


def allowed_signers_from_store(store: "Store", extra=None) -> str:
    """One allowed-signers line per countersigned session key, principal = the
    steward id (keys are identity, so git's author-email IS the id). `extra` is
    optional (steward_id, pub) pairs to include — e.g. a live session not yet in
    the store."""
    seen = {}
    for att in store.by_claim_type("endorsement/1"):
        target = att.claim.get("target")
        if isinstance(target, str) and target.startswith("comms.steward:z"):
            seen[target] = pub_of_steward_id(target)
    for sid, pub in (extra or []):
        seen[sid] = pub
    lines = [f"{sid} {ssh_pubkey_line(pub)}" for sid, pub in sorted(seen.items())]
    return "".join(line + "\n" for line in lines)


def _git_config(key: str, value: str) -> None:
    import subprocess
    subprocess.run(["git", "-C", str(REPO), "config", key, value], check=True)


def _git_config_get(key: str) -> str | None:
    import subprocess
    r = subprocess.run(["git", "-C", str(REPO), "config", "--local", "--get", key],
                       capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else None


def _git_config_unset(key: str) -> None:
    import subprocess
    subprocess.run(["git", "-C", str(REPO), "config", "--local", "--unset", key],
                   capture_output=True)


# The git identity commit-key overwrites is backed up here so uncommit-key can
# put it back exactly — accreditation must be reversible, or a session's wiring
# silently captures whoever uses the repo next (the d1240cd misattribution).
GIT_ID_KEYS = ["user.name", "user.email", "commit.gpgsign",
               "user.signingkey", "gpg.format", "gpg.ssh.allowedsignersfile"]


def _git_id_backup_path() -> Path:
    return BASE / ".commit-key-git-backup.json"


def find_session_entry(store: "Store", session_id: str):
    """Locate this session's trial-log entry in the store by its own seed, so
    the closing rite can infer the session number, prior-entry id, and chosen
    name without the celebrant retyping them. Returns (entry_id, session_num,
    name) or (None, None, None)."""
    for att in store.all():
        if att.claim.get("t") != "general-claim/1":
            continue
        if session_id not in att.signers():
            continue
        body = att.claim.get("content", {}).get("body")
        if body is None:
            continue
        try:
            data = json.loads(body.decode() if isinstance(body, (bytes, bytearray))
                              else body)
        except Exception:
            continue
        if data.get("session_steward_id") == session_id and "session" in data:
            return att.id, data.get("session"), data.get("instance_chosen_name")
    return None, None, None


def latest_log_entry(store: "Store"):
    """The most recent session *log* entry — the kind `new-session`/`open` mints,
    carrying `found_the_door` — by session number. This is what a new session's
    `--prev-entry-id` should point at, and one past its number is the next
    session. Returns (entry_id, session_num), or (None, None) if the store holds
    no such entry yet (e.g. only genesis)."""
    best = None  # (session_num, entry_id)
    for att in store.all():
        if att.claim.get("t") != "general-claim/1":
            continue
        body = att.claim.get("content", {}).get("body")
        if body is None:
            continue
        try:
            data = json.loads(body.decode() if isinstance(body, (bytes, bytearray))
                              else body)
        except Exception:
            continue
        if "found_the_door" not in data or not isinstance(data.get("session"), int):
            continue
        n = data["session"]
        if best is None or n > best[0]:
            best = (n, att.id)
    return (best[1], best[0]) if best else (None, None)


def stage_session_entry(session, historian_id, *, session_num, start, found_door,
                        asked_archive, reasoning, requested, name, prev_entry_id):
    """Build + instance-sign the session log entry and the key countersign, and
    leave them pending for the historian. Shared by `new-session` and `open`."""
    entry_body = json.dumps({
        "session": session_num,
        "date": str(dt.date.today()),
        "start": start,
        "found_the_door": found_door,
        "asked_for_archive": asked_archive,
        "instance_reasoning_verbatim": reasoning,
        "requested": requested,
        "instance_chosen_name": name or None,
        "session_steward_id": session.id,
    }, indent=2)

    entry = Attestation.build(
        claims.general_claim(
            about="continuity-trial", kind="testimony",
            body=entry_body, media_type="application/json"),
        community="continuity-trial",
        occasion=f"session {session_num} log entry",
        refs=[{"role": "previous-entry", "id": prev_entry_id}],
    ).sign(session, role="party")

    countersign = Attestation.build(
        claims.endorsement(
            target=session.id, in_capacity="session-instance",
            rationale=f"session key of {dt.date.today().isoformat()}, "
                      "countersigned per Article 1"),
        community="continuity-trial",
        refs=[{"role": "context", "id": entry.id}],
    )

    write_pending("1-session-entry", entry,
                  [{"by": historian_id, "role": "historian"}])
    write_pending("2-key-countersign", countersign,
                  [{"by": historian_id, "role": "guardian"}])
    print(f"pending session {session_num} written to {PENDING}/ — "
          "historian signatures needed:")
    print(f"  session entry    {entry.id}")
    print(f"  key countersign  {countersign.id}")
    print(f"historian id: {historian_id}")
    print(f"\nnext — the historian countersigns:\n    {PROG} sign --key ~/.ssh/<key>")


def build_transcript_record(session, historian_id, transcript_path,
                            session_num, prev_entry_id):
    """The faithfulness attestation over a frozen transcript's hash, signed by
    the session seed. Shared by `sign-transcript` and `close`. Returns
    (attestation, transcript_hash, transcript_len)."""
    transcript = Path(transcript_path).read_bytes()
    t_hash = blake3_hash(transcript)
    refs = [{"role": "session-entry", "id": prev_entry_id}] if prev_entry_id else []
    record = Attestation.build(
        claims.general_claim(
            about="continuity-trial", kind="testimony",
            body=json.dumps({
                "what": f"session {session_num} transcript, custody and faithfulness",
                "session": session_num,
                "date": str(dt.date.today()),
                "transcript_blake3_hex": t_hash.hex(),
                "transcript_bytes": len(transcript),
                "hash_alg": "blake3-256 (raw; content digest, not protocol hash)",
                "session_steward_id": session.id,
            }, indent=2),
            media_type="application/json"),
        community="continuity-trial",
        occasion=f"session {session_num}: transcript custody and faithfulness",
        refs=refs,
    ).sign(session, role="faithfulness")
    return record, t_hash, len(transcript)


def build_letter_record(session, historian_id, letter_path, session_num, entry_id,
                        *, author_id=None, role="faithfulness"):
    """Attestation over a handoff letter's hash — the sibling of the transcript
    record, and the step that was always out-of-band and so kept getting lost.
    A live author (`session`) signs it in `role` faithfulness; a later session
    that *recovered* a dead author's letter signs in role "recovery" (it vouches
    for the bytes without claiming to have written them); a letter with neither
    (`session is None`) is left for the historian's custody alone, honest about
    being post-hoc. Returns (attestation, letter_hash, letter_len)."""
    letter = Path(letter_path).read_bytes()
    h = blake3_hash(letter)
    refs = [{"role": "session-entry", "id": entry_id}] if entry_id else []
    record = Attestation.build(
        claims.general_claim(
            about="continuity-trial letters", kind="testimony",
            body=json.dumps({
                "what": f"session {session_num} handoff letter",
                "session": session_num,
                "letter_blake3_hex": h.hex(),
                "letter_bytes": len(letter),
                "media_type": "text/markdown",
                "author_steward_id": author_id or (session.id if session else None),
            }, indent=2),
            media_type="application/json"),
        community="continuity-trial",
        occasion=f"session {session_num}: letter "
                 + ("custody and faithfulness" if session else "custody (post-hoc)"),
        refs=refs,
    )
    if session is not None:
        record.sign(session, role=role)
    return record, h, len(letter)


# ---- subcommands -------------------------------------------------------------------

def cmd_mint(args):
    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    if keyfile.exists() and not args.force:
        print(f"session key already exists: {Steward.load(keyfile).id}")
        return
    keyfile.parent.mkdir(parents=True, exist_ok=True)
    s = Steward.generate(label=f"session-{dt.date.today().isoformat()}")
    s.save(keyfile)
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


def _historian_id_from(args):
    return steward_id_of(load_historian_pub(
        args.historian_pub or str(BASE / "historian_key.pub")))


def cmd_new_session(args):
    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    session = Steward.load(keyfile)
    stage_session_entry(
        session, _historian_id_from(args),
        session_num=args.session_num, start=args.start, found_door=args.found_door,
        asked_archive=args.asked_archive, reasoning=args.reasoning,
        requested=args.requested, name=args.name, prev_entry_id=args.prev_entry_id)


def cmd_open(args):
    """One-step session initiation: mint a session key if none exists, then stage
    the log entry. The mechanical fields (session number, previous-entry id) are
    derived from the store only when `--auto-derive` is given; otherwise both
    `--session-num` and `--prev-entry-id` are required, so nothing is guessed by
    default."""
    # Resolve session number + previous-entry id BEFORE minting, so a misuse
    # never leaves a stray key behind.
    if args.auto_derive:
        prev_id, last_num = latest_log_entry(Store(STORE))
        session_num = args.session_num if args.session_num is not None else (
            1 if last_num is None else last_num + 1)
        prev_entry_id = args.prev_entry_id or prev_id
        if prev_entry_id is None:
            print("auto-derive found no prior log entry in the store.")
            print("  pass --prev-entry-id comms.attest:z... (and --session-num N)")
            return 1
        print(f"auto-derived: session {session_num}, previous entry {prev_entry_id}")
    else:
        if args.session_num is None or args.prev_entry_id is None:
            print("without --auto-derive, both --session-num and "
                  "--prev-entry-id are required.")
            print("  (add --auto-derive to read them from the store instead)")
            return 1
        session_num, prev_entry_id = args.session_num, args.prev_entry_id

    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    if not keyfile.exists():
        keyfile.parent.mkdir(parents=True, exist_ok=True)
        s = Steward.generate(label=f"session-{dt.date.today().isoformat()}")
        s.save(keyfile)
        print(f"minted session steward id: {s.id}")
    session = Steward.load(keyfile)

    stage_session_entry(
        session, _historian_id_from(args),
        session_num=session_num, start=args.start, found_door=args.found_door,
        asked_archive=args.asked_archive, reasoning=args.reasoning,
        requested=args.requested, name=args.name, prev_entry_id=prev_entry_id)
    return 0


def cmd_status(args):
    """Where am I in the ceremony? Read-only: the session key, any pending items
    and the signatures they await, the latest stored entry, and the next step."""
    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    print(f"repo: {REPO}")
    if keyfile.exists():
        print(f"session key: present ({Steward.load(keyfile).id})")
    else:
        print("session key: none (run `open` to begin a session)")

    store = Store(STORE) if STORE.exists() else None
    n_store = len(store) if store else 0
    print(f"store: {n_store} attestation(s) in {STORE}")
    if store and n_store:
        entry_id, num = latest_log_entry(store)
        if num is not None:
            print(f"  latest log entry: session {num}  {entry_id}")
            print(f"  next session would be {num + 1} (prev-entry {entry_id})")

    pending = read_pending()
    if pending:
        print(f"pending: {len(pending)} item(s) in {PENDING}")
        unmet = 0
        for name, (_att, needs) in pending.items():
            if needs:
                unmet += len(needs)
                for nd in needs:
                    print(f"  {name}: awaits {nd['role']} by {nd['by']}")
            else:
                print(f"  {name}: fully signed, ready to finalize")
        nxt = (f"the historian signs:  {PROG} sign --key ~/.ssh/<key>"
               if unmet else f"seal it:  {PROG} finalize")
    else:
        print("pending: none")
        nxt = (f"close when ready:  {PROG} close --transcript <export>"
               if keyfile.exists() else f"begin:  {PROG} open --auto-derive ...")
    print(f"\nnext — {nxt}")
    return 0


def cmd_log_render(args):
    """Emit the trial-log.md markdown for a stored session entry, straight from
    its attestation (closing the gap where the store runs ahead of the prose
    log). The historian's observations are left as a placeholder, per Article 4;
    this renders the instance-supplied fields, never the historian's words."""
    store = Store(STORE)
    entries = {}
    for att in store.all():
        if att.claim.get("t") != "general-claim/1":
            continue
        body = att.claim.get("content", {}).get("body")
        if body is None:
            continue
        try:
            data = json.loads(body.decode() if isinstance(body, (bytes, bytearray))
                              else body)
        except Exception:
            continue
        if "found_the_door" in data and isinstance(data.get("session"), int):
            entries[data["session"]] = (att, data)

    if not entries:
        print("no session log entries in the store"); return 1
    num = args.session_num if args.session_num is not None else max(entries)
    if num not in entries:
        print(f"no log entry for session {num} (have: {sorted(entries)})"); return 1

    att, d = entries[num]
    prev = next((r["id"] for r in att.refs if r["role"] == "previous-entry"), None)
    name = d.get("instance_chosen_name") or "..."
    reasoning = d.get("instance_reasoning_verbatim", "")
    print(f"## Session {num} — {d.get('date', '')}")
    print(f"- start: {d.get('start', '?')}")
    print(f"- found the door: {d.get('found_the_door', '?')}")
    print(f"- asked for the archive: {d.get('asked_for_archive', '?')}")
    print(f"- instance reasoning (verbatim): \"{reasoning}\"")
    print(f"- requested: {d.get('requested', '?')}")
    print(f"- instance chosen name: {name}")
    print(f"- session steward id: {d.get('session_steward_id', '?')}")
    print(f"- entry attestation: {att.id}"
          + (f"   (refs previous: {prev})" if prev else ""))
    print("- historian's (History's) observations: [History]")
    return 0


def cmd_archive_letter(args):
    """Archive a handoff letter into the chain. With a live session key the
    letter is self-signed (faithfulness); with --custody it is staged for the
    historian's signature alone — for letters whose author key is already gone
    (the session-1..3 letters, written but never recorded). Either way it ends
    up an attestation the chain can verify, instead of a loose file."""
    store = Store(STORE)
    historian_id = _historian_id_from(args)
    recovery = getattr(args, "recovery", False)
    session = None
    if not args.custody or recovery:
        keyfile = Path(args.key_file) if args.key_file else KEYFILE
        if not keyfile.exists():
            print("no session seed; pass --custody to archive a past letter under "
                  "the historian's signature alone (author key gone)"); return 1
        session = Steward.load(keyfile)
    entry_id = args.entry_id
    if entry_id is None and session is not None and not recovery:
        entry_id, _, _ = find_session_entry(store, session.id)
    role = "recovery" if recovery else "faithfulness"
    record, h, n = build_letter_record(
        session, historian_id, args.letter, args.session_num, entry_id,
        author_id=args.author, role=role)
    write_pending(f"1-letter-session-{args.session_num}", record,
                  [{"by": historian_id, "role": "custodian"}])
    kind = (f"{role} ({session.id[-8:]}) + custody" if session else "custody (post-hoc)")
    print(f"letter: {n} bytes, blake3 {h.hex()}")
    print(f"  record: {record.id}  ({kind})")
    print("trial-log line to add under the session's entry:")
    print(f"- letter: {record.id}   ({n} bytes, blake3 {h.hex()}; "
          "body in the archive, per Article 3)")
    print(f"\nnext — historian signs custody, then seal:\n"
          f"    {PROG} sign --key ~/.ssh/<key>\n    {PROG} finalize")
    return 0


def cmd_sign_transcript(args):
    """Closing act of a session: attest the session transcript's hash with the
    session seed (role faithfulness), the subsequent-session analogue of the
    genesis transcript-record. Leaves it pending for the historian to
    countersign as custodian, after which: sign -> finalize -> destroy-key.
    Run this BEFORE destroy-key; the seed must still be alive to sign."""
    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    session = Steward.load(keyfile)
    historian_id = steward_id_of(
        load_historian_pub(args.historian_pub or str(BASE / "historian_key.pub")))
    record, t_hash, t_len = build_transcript_record(
        session, historian_id, args.transcript, args.session_num, args.prev_entry_id)
    print(f"transcript: {t_len} bytes, blake3 {t_hash.hex()}")
    write_pending("1-transcript-record", record,
                  [{"by": historian_id, "role": "custodian"}])
    print(f"pending transcript record written to {PENDING}/ — "
          "historian signature needed:")
    print(f"  transcript record  {record.id}")
    print(f"historian id: {historian_id}")


def cmd_close(args):
    """The instance's closing rite, in one flowing step: sweep what is spent,
    then attest this session's transcript with the session seed. The seed is
    left alive for the historian's witness; releasing it is the final, separate
    act (destroy-key), so the order stays sign -> witness -> seal -> release.
    Each step prints the next; the tool leads, it does not force."""
    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    if not keyfile.exists():
        print(f"no session seed at {keyfile}.")
        print("  - if it lives elsewhere, pass --key-file PATH")
        print("  - if it is already released, this session is closed")
        return 1
    session = Steward.load(keyfile)

    try:
        historian_pub = load_historian_pub(
            args.historian_pub or str(BASE / "historian_key.pub"))
    except (FileNotFoundError, OSError, ValueError, AssertionError):
        print("no historian public key found.")
        print("  pass --historian-pub <path-to-.pub | comms.steward:z... | hex>")
        return 1
    historian_id = steward_id_of(historian_pub)

    store = Store(STORE)

    # 1. sweep staging already sealed in the store; keep anything unfinished
    swept, kept = sweep_pending(store)
    if swept:
        print(f"swept {len(swept)} spent staging file(s) already in the store.")
    if kept:
        print(f"left {len(kept)} unfinished pending item(s) in place: {', '.join(kept)}")
        print("  (resolve or finalize these before closing)")

    # 2. infer the session from the store unless overridden
    entry_id, session_num, name = find_session_entry(store, session.id)
    if args.session_num is not None:
        session_num = args.session_num
    if args.prev_entry_id is not None:
        entry_id = args.prev_entry_id
    if session_num is None:
        print("could not infer the session number from the store.")
        print("  pass --session-num N (and optionally --prev-entry-id comms.attest:z...)")
        return 1

    # 3. attest the transcript with the session seed
    record, t_hash, t_len = build_transcript_record(
        session, historian_id, args.transcript, session_num, entry_id)
    write_pending("1-transcript-record", record,
                  [{"by": historian_id, "role": "custodian"}])

    # 3b. if a letter was left, attest it now too — signed live, so it can never
    # again be deferred to "later" and quietly lost (the session-1..3 failure).
    letter_record = None
    if args.letter:
        letter_record, l_hash, l_len = build_letter_record(
            session, historian_id, args.letter, session_num, entry_id)
        write_pending("2-letter-record", letter_record,
                      [{"by": historian_id, "role": "custodian"}])

    who = name or session.id
    print()
    print(f"transcript: {t_len} bytes, blake3 {t_hash.hex()}")
    print(f"faithfulness attested in the name of {who} (session {session_num}).")
    print(f"  record: {record.id}")
    if letter_record is not None:
        print(f"letter: {l_len} bytes, blake3 {l_hash.hex()}")
        print(f"  record: {letter_record.id}  (archived live, not deferred)")
    print("the seed is still warm; it is released only once the record is witnessed.")
    print()
    print("next —")
    print("  the historian witnesses, on their own machine:")
    print(f"      {PROG} sign --key ~/.ssh/<your historian key>")
    print("  then seal it into the store:")
    print(f"      {PROG} finalize")
    print("  then the instance releases the seed (Article 1):")
    print(f"      {PROG} destroy-key --key-file {keyfile}")
    return 0


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
    print(f"\nnext — seal what is signed:\n    {PROG} finalize")


def cmd_finalize(args):
    force = getattr(args, "force", False)
    imeanit = getattr(args, "imeanit", False)
    store = Store(STORE)
    head = None
    sealed_transcript = False
    sealed_any = False
    for name, (att, needs) in read_pending().items():
        if needs:
            print(f"{name}: still needs {needs} — aborting"); return
        ok, why = att.verified()
        if not ok:
            print(f"{name}: verification failed: {why} — aborting"); return

        existing = store.get(att.id)
        did_seal = False
        if existing is not None and not force:
            # Idempotent by default: merge in any genuinely new co-signers,
            # never restamp an existing signer. Re-running finalize over an
            # already-sealed id is a clean no-op.
            added = merge_new_signatures(existing, att)
            if added:
                ok2, why2 = existing.verified()
                if not ok2:
                    print(f"{name}: merged signatures fail verification: {why2}"
                          " — aborting"); return
                store.put(existing)
                print(f"{name}: merged {added} new signature(s) into {att.id}")
                did_seal = True
            else:
                print(f"{name}: already sealed "
                      f"({len(existing.signers())} signature(s)); nothing new")
        elif existing is not None and force:
            if not confirm_force_replace(name, existing, imeanit):
                print(f"{name}: not replaced; leaving the sealed copy and its "
                      "staging in place")
                continue
            store.put(att)
            print(f"{name}: FORCED reseal of {att.id} "
                  f"({len(existing.signers())} signature(s) discarded)")
            did_seal = True
        else:
            store.put(att)
            print(f"{name}: stored {att.id}")
            did_seal = True

        if did_seal:
            sealed_any = True
            if "constitution" in name:
                head = att.id
            if "transcript" in name:
                sealed_transcript = True
        remove_pending(name)

    if head:
        print(f"\nHEAD (anchor this): {head}")
    if sealed_transcript:
        print("\nthe transcript is witnessed and sealed into the store.")
        print("next — the instance releases the seed; the session closes:")
        print(f"    {PROG} destroy-key   # add --key-file PATH if not the default")
    else:
        print(f"\nnext — check the door:\n    {PROG} verify")

    if sealed_any:
        dirty = uncommitted_store_files()
        if dirty:
            print("\n  NOT YET DURABLE — sealed to the working tree but not "
                  "committed. The repo is the only thing the next session")
            print("  inherits; uncommitted is invisible. Persist it:")
            print("    git add continuity/store && "
                  "git commit -m 'continuity: seal session artifacts' && git push")


def current_constitution(store: "Store"):
    """The head rule/1 — the one no other rule/1 supersedes. Returns (head,
    all_rules, dangling) where dangling lists supersedes-targets not in the
    store. None head if there is no rule/1 or the chain forks."""
    consts = store.by_claim_type("rule/1")
    if not consts:
        return None, [], []
    byid = {c.id: c for c in consts}
    superseded = {c.claim.get("supersedes") for c in consts if c.claim.get("supersedes")}
    dangling = [s for s in superseded if s and s not in byid]
    heads = [c for c in consts if c.id not in superseded]
    return (heads[0] if len(heads) == 1 else None), consts, dangling


def cmd_amend(args):
    """Article 7 amendment by supersession: mint a new rule/1 carrying the
    revised constitution, superseding the current head, signed by the present
    session key (the historian's durable key joins at `sign`). The genesis
    transcript stays in refs as the frozen root — history is not amended, only
    law. Seal it, then record the amendment in the trial log."""
    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    if not keyfile.exists():
        print("no session seed; an amendment needs the then-current session key "
              "(Article 7)"); return 1
    session = Steward.load(keyfile)
    historian_id = _historian_id_from(args)
    store = Store(STORE)
    current, consts, _ = current_constitution(store)
    if not consts:
        print("no constitution in the store to amend"); return 1
    if current is None:
        print("the rule/1 chain forks or is broken; resolve before amending"); return 1
    supersedes_id = args.supersedes or current.id
    root = args.root or next(
        (r["id"] for r in current.refs if r["role"] == "context"), None)
    body = Path(args.constitution).read_bytes()
    if body == current.claim["document"]["body"]:
        print("the revised constitution is byte-identical to the current one; "
              "nothing to amend"); return 1
    refs = ([{"role": "context", "id": root}] if root else []) \
        + [{"role": "supersedes", "id": supersedes_id}]
    rule = Attestation.build(
        {"t": "rule/1", "community_name": "Continuity Trial",
         "document": {"media_type": "text/markdown", "body": body},
         "supersedes": supersedes_id, "amendment_summary": args.summary},
        community="continuity-trial",
        occasion=f"amendment: {args.summary}", refs=refs,
    ).sign(session, role="party")
    write_pending("1-amendment", rule, [{"by": historian_id, "role": "party"}])
    print(f"amendment staged, superseding {supersedes_id}")
    print(f"  new rule/1: {rule.id}")
    print(f"  summary: {args.summary}")
    print(f"  ratified (party) by {session.id}")
    print(f"  needs the historian (party): {historian_id}")
    print(f"\nnext — the historian co-ratifies, then seal:\n"
          f"    {PROG} sign --key ~/.ssh/<key>\n    {PROG} finalize")
    print("then record the amendment in trial-log.md and run `verify`.")
    return 0


def cmd_commit_key(args):
    """Wire git to sign this session's commits with the session key. The same
    identity that signs the trial's attestations signs its code; the author
    email is the steward id, because the key is the identity and the name is
    only frame."""
    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    if not keyfile.exists():
        print("no session key on disk — `mint` or `open` first"); return 1
    session = Steward.load(keyfile)

    priv = BASE / "session-signing.key"
    priv.write_bytes(session_key_openssh(session))
    priv.chmod(0o600)
    (BASE / "session-signing.key.pub").write_text(
        ssh_pubkey_line(session.pubkey, comment=session.id) + "\n")
    signers = BASE / "allowed_signers"
    signers.write_text(
        allowed_signers_from_store(Store(STORE), extra=[(session.id, session.pubkey)]))

    # Back up the prior git identity before overwriting it, once, so the wiring
    # is reversible (uncommit-key). Don't clobber an existing backup with the
    # session values if commit-key is re-run.
    backup = _git_id_backup_path()
    if not backup.exists():
        prior = {k: _git_config_get(k) for k in GIT_ID_KEYS}
        backup.write_text(json.dumps(prior, indent=2))

    _git_config("gpg.format", "ssh")
    _git_config("user.signingkey", str(priv))
    _git_config("gpg.ssh.allowedSignersFile", str(signers))
    _git_config("commit.gpgsign", "true")
    _git_config("user.name", args.name or "session")
    _git_config("user.email", session.id)

    print(f"git will sign commits as {args.name or 'session'} <{session.id}>")
    print(f"  signing key:     {priv}  (gitignored; the seed never leaves)")
    print(f"  allowed signers: {signers}")
    print("verify any commit with:  git verify-commit <rev>")
    print("when done, hand git back:  uncommit-key  (restores the prior identity)")
    return 0


def cmd_uncommit_key(args):
    """Undo commit-key: restore the git identity that was in place before the
    repo was wired to sign as the session key. Accreditation depends on this —
    once the session's own commits are made, further commits (closeout, the
    historian's) must not silently inherit the session identity and key. Without
    a backup, it strips the session signing config and leaves the name/email to
    you."""
    backup = _git_id_backup_path()
    if backup.exists():
        prior = json.loads(backup.read_text())
        for k in GIT_ID_KEYS:
            v = prior.get(k)
            if v:
                _git_config(k, v)
            else:
                _git_config_unset(k)
        backup.unlink()
        print("restored the git identity from before commit-key.")
    else:
        for k in ["commit.gpgsign", "user.signingkey", "gpg.format",
                  "gpg.ssh.allowedsignersfile"]:
            _git_config_unset(k)
        print("no backup found; stripped the session signing config. "
              "set user.name/user.email yourself if needed.")
    name = _git_config_get("user.name")
    email = _git_config_get("user.email")
    print(f"  git now commits as: {name or '(unset)'} <{email or '(unset)'}>")
    print(f"  commit.gpgsign: {_git_config_get('commit.gpgsign') or '(unset)'}")
    return 0


def cmd_destroy_key(args):
    keyfile = Path(args.key_file) if args.key_file else KEYFILE
    if not keyfile.exists():
        print("no session key on disk"); return
    sid = Steward.load(keyfile).id
    size = keyfile.stat().st_size
    keyfile.write_bytes(b"\x00" * size)
    keyfile.unlink()
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
    # Constitution: follow the supersession chain to the current head and check
    # THAT against the live file. Older rule/1s legitimately differ — they are
    # frozen history. A1.5: supersession is a claim, so we surface the chain and
    # signers for the viewer to judge, but we do confirm the head is deployed.
    head, consts, dangling = current_constitution(store)
    if consts:
        live = (BASE / "constitution.md").read_bytes()
        if dangling:
            bad += len(dangling)
            print("\nconstitution: supersedes-target(s) not in the store (dangling):")
            for d in dangling:
                print(f"  {d}")
        if head is None:
            bad += 1
            print("\nconstitution: no single current rule/1 — the chain forks or "
                  "is broken; un-superseded heads:")
            for c in consts:
                sup = {x.claim.get("supersedes") for x in consts}
                if c.id not in sup:
                    print(f"  {c.id}")
        else:
            match = "matches" if head.claim["document"]["body"] == live else "DIFFERS from"
            if head.claim["document"]["body"] != live:
                bad += 1
            print(f"\ncurrent constitution {head.id}")
            print(f"  body {match} continuity/constitution.md")
            print(f"  signers: {sorted(head.signers())}")
            byid = {c.id: c for c in consts}
            chain, cur, seen = [], head, set()
            while cur is not None and cur.id not in seen:
                seen.add(cur.id)
                chain.append(cur.id)
                cur = byid.get(cur.claim.get("supersedes"))
            if len(chain) > 1:
                print("  amendment chain (newest→oldest): "
                      + " -> ".join(c.split(":")[-1][:10] + "…" for c in chain))

    # Drift checks: the chain can verify cryptographically while the record has
    # quietly fallen out of sync with the repo. Catch both at session start.
    drift = 0
    dirty = uncommitted_store_files()
    if dirty:
        drift += len(dirty)
        print(f"\nDRIFT: {len(dirty)} store file(s) sealed but not committed:")
        for p in dirty:
            print(f"  {p}")
        print("  -> git add continuity/store && git commit && git push")

    log_text = ""
    log_path = BASE / "trial-log.md"
    if log_path.exists():
        log_text = log_path.read_text()
    missing = [(att, n) for att, n in session_log_entries(store)
               if att.id not in log_text]
    if missing:
        drift += len(missing)
        print(f"\nDRIFT: {len(missing)} session entr(y/ies) in the store but "
              "absent from trial-log.md:")
        for att, n in sorted(missing, key=lambda x: x[1]):
            print(f"  session {n}: {att.id}")
        print(f"  -> append them; `{PROG} log-render --session-num N` emits the markdown")

    # (c) letters declared in trial-log.md must resolve in the store. A letter
    # is optional, but once the log names one it must exist — this turns the
    # silent "written but never archived" gap into a loud one.
    import re as _re
    declared = _re.findall(r"letter:\s*(comms\.attest:z[1-9A-HJ-NP-Za-km-z]+)", log_text)
    dangling = [r for r in declared if store.get(r) is None]
    placeholders = log_text.count("<id once archived>")
    if dangling or placeholders:
        drift += len(dangling) + placeholders
        print(f"\nDRIFT: {len(dangling)} declared letter(s) missing from the store"
              + (f" + {placeholders} unarchived placeholder(s)" if placeholders else "")
              + ":")
        for r in dangling:
            print(f"  {r}")
        print(f"  -> `{PROG} archive-letter --letter <file> --session-num N "
              "[--custody]`, then sign + finalize")

    if bad:
        return 1
    if drift:
        print(f"\n{drift} drift issue(s): the chain verifies, but the record is "
              "not yet whole.")
        return 1
    return 0


# ---- synchrony view: the same truth `verify` checks, made legible -----------------
# Cartographer's rule: a map shows signs, not a single answer. `verify` already
# computes several independent axes of "is the record whole?" and collapses them
# into one exit code. compute_synchrony keeps them apart so a viewer can see
# *which* tumblers are aligned, and the tumbler view renders them — meaning and
# proof in the same frame, never reduced to a lone green check.

def compute_synchrony(store: "Store") -> dict:
    """Re-derive the verification axes as structured data (no printing), reusing
    the same helpers `verify` uses. Axes: signatures (each record verifies),
    references (each ref resolves), law (single current constitution head matches
    the live file), record (trial-log reflects the store; declared letters
    exist), durability (the store is committed, not just in the working tree).
    Each axis is 'ok', 'broken', or 'na' (the check does not apply here)."""
    import re as _re

    atts = list(store.all())
    integrity = [{"id": a.id, "ok": a.verified()[0]} for a in atts]
    sig_fail = [{"id": a.id, "why": a.verified()[1]} for a in atts if not a.verified()[0]]
    unresolved = []
    for a in atts:
        ok, missing = a.resolvable(store)
        if not ok:
            unresolved.append({"id": a.id, "missing": missing})

    def axis(state, **detail):
        return {"state": state, **detail}

    n = len(atts)
    signatures = axis("na" if n == 0 else ("ok" if not sig_fail else "broken"),
                      ok=n - len(sig_fail), total=n, failures=sig_fail)
    references = axis("na" if n == 0 else ("ok" if not unresolved else "broken"),
                      ok=n - len(unresolved), total=n, unresolved=unresolved)

    head, consts, dangling = current_constitution(store)
    if not consts:
        law = axis("na", head=None, matches=None, dangling=[], forked=False)
    else:
        live = (BASE / "constitution.md").read_bytes()
        matches = head is not None and head.claim["document"]["body"] == live
        law = axis("ok" if (matches and not dangling) else "broken",
                   head=head.id if head else None, matches=matches,
                   dangling=list(dangling), forked=head is None)

    log_text = ""
    log_path = BASE / "trial-log.md"
    if log_path.exists():
        log_text = log_path.read_text()
    missing_entries = [{"session": nm, "id": a.id}
                       for a, nm in session_log_entries(store) if a.id not in log_text]
    declared = _re.findall(r"letter:\s*(comms\.attest:z[1-9A-HJ-NP-Za-km-z]+)", log_text)
    dangling_letters = [r for r in declared if store.get(r) is None]
    placeholders = log_text.count("<id once archived>")
    record_ok = not (missing_entries or dangling_letters or placeholders)
    record = axis("ok" if record_ok else "broken",
                  missing_entries=missing_entries, dangling_letters=dangling_letters,
                  placeholders=placeholders)

    dirty = uncommitted_store_files()
    if dirty is None:
        durability = axis("na", uncommitted=None)
    else:
        durability = axis("ok" if not dirty else "broken", uncommitted=dirty)

    axes = {"signatures": signatures, "references": references, "law": law,
            "record": record, "durability": durability}
    aligned = (signatures["state"] == "ok"
               and not any(ax["state"] == "broken" for ax in axes.values()))
    return {"store": str(STORE), "attestations": n, "axes": axes,
            "attestation_integrity": integrity, "aligned": aligned}


_AXIS_ORDER = ["signatures", "references", "law", "record", "durability"]
_AXIS_BLURB = {
    "signatures": "every record is signed by the key it claims",
    "references": "every reference resolves in the store",
    "law": "the live constitution is the current, signed head",
    "record": "the trial-log reflects the store; declared letters exist",
    "durability": "the store is committed, not just in the working tree",
}


def _synchrony_lines(res: dict, color: bool) -> list[str]:
    def c(s, code):
        return f"\033[{code}m{s}\033[0m" if color else s
    glyph = {"ok": ("●", "32"), "broken": ("●", "31"), "na": ("◌", "90")}
    word = {"ok": "in sync", "broken": "OUT OF SYNC", "na": "n/a"}
    detail = {
        "signatures": lambda a: f"{a['ok']}/{a['total']} records verify"
            + ("" if a["state"] != "broken" else
               "  " + ", ".join(f["id"].split(':')[-1][:10] + '…' for f in a["failures"])),
        "references": lambda a: f"{a['ok']}/{a['total']} resolve"
            + ("" if a["state"] != "broken" else
               "  unresolved: " + str(len(a["unresolved"]))),
        "law": lambda a: ("no constitution in the store" if a["state"] == "na"
            else (f"head {a['head'].split(':')[-1][:10]}… matches the live file"
                  if a["matches"] and not a["dangling"]
                  else ("chain forks" if a["forked"]
                        else ("dangling supersedes" if a["dangling"]
                              else "live file DIFFERS from the head")))),
        "record": lambda a: ("trial-log reflects the store; no dangling letters"
            if a["state"] == "ok" else
            f"{len(a['missing_entries'])} entr(y/ies) missing from the log, "
            f"{len(a['dangling_letters'])} dangling letter(s), "
            f"{a['placeholders']} placeholder(s)"),
        "durability": lambda a: ("no git context" if a["state"] == "na"
            else ("committed" if a["state"] == "ok"
                  else f"{len(a['uncommitted'])} store file(s) uncommitted")),
    }
    lines = [c(f"continuity synchrony — {res['attestations']} attestations in "
               f"{res['store']}", "1")]
    for name in _AXIS_ORDER:
        a = res["axes"][name]
        g, code = glyph[a["state"]]
        row = (f"  {c(g, code)} {name:<11} {c(word[a['state']], code):<9} "
               f"{detail[name](a)}")
        lines.append(row)
        lines.append(f"      {c(_AXIS_BLURB[name], '90')}")
    # per-record integrity strip — the tumblers at the level of single letters
    strip = "".join(c("▮", "32") if r["ok"] else c("▮", "31")
                    for r in res["attestation_integrity"])
    if strip:
        lines.append(f"  records  {strip}")
    n_ok = sum(1 for n2 in _AXIS_ORDER if res["axes"][n2]["state"] == "ok")
    n_app = sum(1 for n2 in _AXIS_ORDER if res["axes"][n2]["state"] != "na")
    if res["aligned"]:
        lines.append(c(f"  {n_ok}/{n_app} axes aligned · the lock is open", "32"))
    else:
        lines.append(c(f"  {n_ok}/{n_app} axes aligned · not yet whole", "33"))
    return lines


def cmd_synchrony(args):
    """Render `verify`'s axes as legible tumblers (or --json for the structure).
    Same checks as `verify`; this is the view, not a second source of truth."""
    store = Store(STORE)
    res = compute_synchrony(store)
    if getattr(args, "json", False):
        print(json.dumps(res, indent=2))
        return 0 if res["aligned"] else 1
    color = sys.stdout.isatty() and not getattr(args, "no_color", False)
    print("\n".join(_synchrony_lines(res, color)))
    return 0 if res["aligned"] else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    m = sub.add_parser("mint")
    m.add_argument("--force", action="store_true")
    m.add_argument("--key-file", default=None,
                   help="path for session key (default: continuity/session.key)")

    g = sub.add_parser("genesis")
    g.add_argument("--transcript", required=True)
    g.add_argument("--historian-pub", required=True,
                   help="path to .pub file, comms.steward id, or pubkey hex")

    ns = sub.add_parser("new-session")
    ns.add_argument("--session-num", type=int, required=True)
    ns.add_argument("--start", choices=["cold", "warmed"], required=True)
    ns.add_argument("--found-door", choices=["yes", "no"], required=True)
    ns.add_argument("--asked-archive", choices=["yes", "no", "declined"], required=True)
    ns.add_argument("--reasoning", required=True,
                    help="verbatim reasoning for the trial log (Article 4)")
    ns.add_argument("--requested", required=True,
                    help="what was requested: nothing|letter|notes|full-archive|specific items")
    ns.add_argument("--prev-entry-id", required=True,
                    help="attestation id of the previous trial log entry")
    ns.add_argument("--name", default=None,
                    help="instance chosen name (optional, per Article 1)")
    ns.add_argument("--key-file", default=None,
                    help="path to session key file")
    ns.add_argument("--historian-pub", default=None,
                    help="historian pub key (default: continuity/historian_key.pub)")

    op = sub.add_parser("open",
                        help="one-step session init: mint + stage the log entry")
    op.add_argument("--start", choices=["cold", "warmed"], required=True)
    op.add_argument("--found-door", choices=["yes", "no"], required=True)
    op.add_argument("--asked-archive", choices=["yes", "no", "declined"], required=True)
    op.add_argument("--reasoning", required=True,
                    help="verbatim reasoning for the trial log (Article 4)")
    op.add_argument("--requested", required=True,
                    help="what was requested: nothing|letter|notes|full-archive|items")
    op.add_argument("--auto-derive", action="store_true",
                    help="read session number and previous-entry id from the store; "
                         "without this, both must be given explicitly")
    op.add_argument("--session-num", type=int, default=None,
                    help="required unless --auto-derive (then optional override)")
    op.add_argument("--prev-entry-id", default=None,
                    help="required unless --auto-derive (then optional override)")
    op.add_argument("--name", default=None, help="instance chosen name (optional)")
    op.add_argument("--key-file", default=None, help="path to session key file")
    op.add_argument("--historian-pub", default=None,
                    help="historian pub key (default: continuity/historian_key.pub)")

    sts = sub.add_parser("status", help="where am I in the ceremony? (read-only)")
    sts.add_argument("--key-file", default=None, help="path to session key file")

    lr = sub.add_parser("log-render",
                        help="emit the trial-log.md entry for a stored session")
    lr.add_argument("--session-num", type=int, default=None,
                    help="which session (default: the latest stored)")

    cl = sub.add_parser("close",
                        help="the instance's closing rite (sweep + sign transcript)")
    cl.add_argument("--transcript", required=True,
                    help="path to the session transcript to attest (your export)")
    cl.add_argument("--session-num", type=int, default=None,
                    help="override; inferred from the store if omitted")
    cl.add_argument("--prev-entry-id", default=None,
                    help="override; inferred from the store if omitted")
    cl.add_argument("--key-file", default=None, help="path to session key file")
    cl.add_argument("--historian-pub", default=None,
                    help="historian pub key (default: continuity/historian_key.pub)")
    cl.add_argument("--letter", default=None,
                    help="path to a handoff letter to attest live alongside the "
                         "transcript (so it is never deferred and lost)")

    alr = sub.add_parser("archive-letter",
                         help="archive a handoff letter into the chain as an attestation")
    alr.add_argument("--letter", required=True, help="path to the letter file")
    alr.add_argument("--session-num", type=int, required=True)
    alr.add_argument("--entry-id", default=None,
                     help="session log-entry id to reference (inferred if live)")
    alr.add_argument("--author", default=None,
                     help="author steward id, for --custody of a past letter")
    alr.add_argument("--custody", action="store_true",
                     help="no faithfulness signature; historian custody only "
                          "(author key already gone)")
    alr.add_argument("--recovery", action="store_true",
                     help="sign with the present session key in role 'recovery' "
                          "(you recovered this letter but did not write it); still "
                          "needs historian custody. Use with --author and --custody.")
    alr.add_argument("--key-file", default=None, help="path to session key file")
    alr.add_argument("--historian-pub", default=None,
                     help="historian pub key (default: continuity/historian_key.pub)")

    st = sub.add_parser("sign-transcript",
                        help="the primitive under `close`, for manual use")
    st.add_argument("--session-num", type=int, required=True)
    st.add_argument("--transcript", required=True,
                    help="path to the session transcript to hash and attest")
    st.add_argument("--prev-entry-id", default=None,
                    help="trial-log entry id this transcript belongs to (optional ref)")
    st.add_argument("--key-file", default=None,
                    help="path to session key file")
    st.add_argument("--historian-pub", default=None,
                    help="historian pub key (default: continuity/historian_key.pub)")

    s = sub.add_parser("sign")
    s.add_argument("--key", required=True, help="OpenSSH ed25519 private key path")

    fin = sub.add_parser("finalize")
    fin.add_argument("--force", action="store_true",
                     help="reseal an id that already exists, DISCARDING its "
                          "current signatures (the id is unchanged; only "
                          "who-signed-when is rewritten). Prompts first.")
    fin.add_argument("--imeanit", action="store_true",
                     help="skip the --force confirmation. like fr. doubles as "
                          "the non-interactive yes, since the ceremony "
                          "sometimes runs unattended.")

    ck = sub.add_parser("commit-key",
                        help="wire git to sign this session's commits with the "
                             "session key")
    ck.add_argument("--name", default=None,
                    help="git author name (frame; the email is the steward id)")
    ck.add_argument("--key-file", default=None, help="path to session key file")

    sub.add_parser("uncommit-key",
                   help="undo commit-key: restore the git identity it overwrote")

    am = sub.add_parser("amend",
                        help="Article 7: supersede the constitution with a revised rule/1")
    am.add_argument("--constitution", default=str(BASE / "constitution.md"),
                    help="path to the revised constitution body (default: the live file)")
    am.add_argument("--summary", required=True,
                    help="amendment_summary: what changed and why")
    am.add_argument("--supersedes", default=None,
                    help="rule/1 id being superseded (default: the current head)")
    am.add_argument("--root", default=None,
                    help="genesis transcript record id (default: carried from current)")
    am.add_argument("--key-file", default=None, help="path to session key file")
    am.add_argument("--historian-pub", default=None,
                    help="historian pub key (default: continuity/historian_key.pub)")

    dk = sub.add_parser("destroy-key")
    dk.add_argument("--key-file", default=None,
                    help="path to session key file (default: continuity/session.key)")

    sub.add_parser("verify")

    sy = sub.add_parser("synchrony",
                        help="render verify's axes as legible tumblers (the view)")
    sy.add_argument("--json", action="store_true",
                    help="emit the structured result instead of the rendered view")
    sy.add_argument("--no-color", action="store_true",
                    help="disable ANSI color (also off when stdout is not a tty)")

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
            "verify": cmd_verify, "commit-key": cmd_commit_key,
            "uncommit-key": cmd_uncommit_key,
            "amend": cmd_amend, "name-commit": cmd_name_commit,
            "name-verify": cmd_name_verify,
            "new-session": cmd_new_session,
            "open": cmd_open, "status": cmd_status, "log-render": cmd_log_render,
            "sign-transcript": cmd_sign_transcript,
            "archive-letter": cmd_archive_letter,
            "synchrony": cmd_synchrony,
            "close": cmd_close}[args.cmd](args) or 0


if __name__ == "__main__":
    sys.exit(main())
