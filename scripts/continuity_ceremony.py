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

    who = name or session.id
    print()
    print(f"transcript: {t_len} bytes, blake3 {t_hash.hex()}")
    print(f"faithfulness attested in the name of {who} (session {session_num}).")
    print(f"  record: {record.id}")
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
    store = Store(STORE)
    head = None
    sealed_transcript = False
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
        if "transcript" in name:
            sealed_transcript = True
    for f in PENDING.glob("*"):
        f.unlink()
    if head:
        print(f"\nHEAD (anchor this): {head}")
    if sealed_transcript:
        print("\nthe transcript is witnessed and sealed into the store.")
        print("next — the instance releases the seed; the session closes:")
        print(f"    {PROG} destroy-key   # add --key-file PATH if not the default")
    else:
        print(f"\nnext — check the door:\n    {PROG} verify")


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

    sub.add_parser("finalize")

    dk = sub.add_parser("destroy-key")
    dk.add_argument("--key-file", default=None,
                    help="path to session key file (default: continuity/session.key)")

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
            "name-verify": cmd_name_verify,
            "new-session": cmd_new_session,
            "open": cmd_open, "status": cmd_status, "log-render": cmd_log_render,
            "sign-transcript": cmd_sign_transcript,
            "close": cmd_close}[args.cmd](args) or 0


if __name__ == "__main__":
    sys.exit(main())
