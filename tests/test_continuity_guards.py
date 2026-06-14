"""Guards on the continuity ceremony's finalize/verify steps.

These cover the three failure modes that a tired or interrupted operator hit
in practice:

  1. re-running `finalize` over an already-sealed attestation silently
     restamped it (new `signed_at`, same id) — a corrupted-looking diff that
     rewrote faithfulness metadata. Now: merge-by-default (idempotent), with an
     explicit `--force`/`--imeanit` ladder for the rare destructive reseal.
  2. `finalize` sealed to the working tree but nothing committed, so the next
     fresh checkout never inherited it. Now: a durability notice / drift check.
  3. trial-log.md fell behind the store. Now: `verify` flags entries present in
     the store but absent from the log.
"""

import importlib.util
import json
from argparse import Namespace
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture(scope="session")
def cc():
    """The continuity_ceremony script, imported as a module."""
    spec = importlib.util.spec_from_file_location(
        "continuity_ceremony", REPO_ROOT / "scripts" / "continuity_ceremony.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


@pytest.fixture
def trial(cc, tmp_path, monkeypatch):
    """Redirect the ceremony's paths at a throwaway (non-git) trial dir."""
    base = tmp_path / "continuity"
    store = base / "store"
    pending = base / "pending"
    store.mkdir(parents=True)
    pending.mkdir(parents=True)
    monkeypatch.setattr(cc, "REPO", tmp_path)        # non-git -> git checks skip
    monkeypatch.setattr(cc, "BASE", base)
    monkeypatch.setattr(cc, "STORE", store)
    monkeypatch.setattr(cc, "PENDING", pending)
    return cc, base, store, pending


def _entry(cc, session_steward, session_num):
    """A session log entry attestation, signed by the session key only."""
    body = json.dumps({
        "session": session_num,
        "date": "2026-06-13",
        "start": "cold",
        "found_the_door": "yes",
        "asked_for_archive": "yes",
        "instance_reasoning_verbatim": "test",
        "requested": "letter",
        "instance_chosen_name": "Tester",
        "session_steward_id": session_steward.id,
    }, indent=2)
    return cc.Attestation.build(
        cc.claims.general_claim(about="continuity-trial", kind="testimony",
                                body=body, media_type="application/json"),
        community="continuity-trial", occasion=f"session {session_num} log entry",
        refs=[{"role": "previous-entry", "id": "comms.attest:z" + "1" * 44}],
    ).sign(session_steward, role="party")


def _stored_bytes(store, att):
    return (store / (att.id.split(":")[-1] + ".cbor")).read_bytes()


# --- guard 1: finalize idempotency / force ladder --------------------------------

def test_rerun_finalize_is_a_noop_no_restamp(trial):
    """The exact regression: a sealed entry re-finalized with a fresh-timestamp
    signature from the *same* signer must not be rewritten."""
    cc, base, store_dir, pending = trial
    session = cc.Steward.generate()
    historian = cc.Steward.generate()

    entry = _entry(cc, session, 1)
    sealed = cc.Attestation.from_cbor(entry.to_cbor())   # same core -> same id
    sealed.sign(historian, role="historian", signed_at="2026-06-13T13:56:11Z")
    cc.Store(store_dir).put(sealed)
    before = _stored_bytes(store_dir, sealed)

    # stage a copy that re-signs as the historian with a *new* timestamp
    restage = cc.Attestation.from_cbor(entry.to_cbor())
    restage.sign(historian, role="historian", signed_at="2026-06-14T09:00:00Z")
    cc.write_pending("1-session-entry", restage, [])

    cc.cmd_finalize(Namespace(force=False, imeanit=False))

    assert _stored_bytes(store_dir, sealed) == before        # untouched
    assert not list(pending.glob("*"))                       # staging swept


def test_finalize_merges_a_new_cosigner(trial):
    """A genuinely new signer is merged in; the prior signature is preserved."""
    cc, base, store_dir, pending = trial
    session = cc.Steward.generate()
    historian = cc.Steward.generate()

    entry = _entry(cc, session, 1)
    cc.Store(store_dir).put(cc.Attestation.from_cbor(entry.to_cbor()))  # session only

    cosigned = cc.Attestation.from_cbor(entry.to_cbor())
    cosigned.sign(historian, role="historian", signed_at="2026-06-14T09:00:00Z")
    cc.write_pending("1-session-entry", cosigned, [])

    cc.cmd_finalize(Namespace(force=False, imeanit=False))

    out = cc.Store(store_dir).get(entry.id)
    assert out.signers() == {session.id, historian.id}
    assert out.verified()[0]


def test_force_with_imeanit_replaces_signatures(trial):
    """--force --imeanit restamps non-interactively."""
    cc, base, store_dir, pending = trial
    session = cc.Steward.generate()
    historian = cc.Steward.generate()

    entry = _entry(cc, session, 1)
    sealed = cc.Attestation.from_cbor(entry.to_cbor())
    sealed.sign(historian, role="historian", signed_at="2026-06-13T13:56:11Z")
    cc.Store(store_dir).put(sealed)

    restage = cc.Attestation.from_cbor(entry.to_cbor())
    restage.sign(historian, role="historian", signed_at="2026-06-14T09:00:00Z")
    cc.write_pending("1-session-entry", restage, [])

    cc.cmd_finalize(Namespace(force=True, imeanit=True))

    out = cc.Store(store_dir).get(entry.id)
    stamps = [s["signed_at"] for s in out.signatures if s["by"] == historian.id]
    assert stamps == ["2026-06-14T09:00:00Z"]               # replaced, not appended


def test_force_without_imeanit_aborts_noninteractively(trial):
    """No --imeanit and no tty (pytest) -> refuse, leave the seal untouched."""
    cc, base, store_dir, pending = trial
    session = cc.Steward.generate()
    historian = cc.Steward.generate()

    entry = _entry(cc, session, 1)
    sealed = cc.Attestation.from_cbor(entry.to_cbor())
    sealed.sign(historian, role="historian", signed_at="2026-06-13T13:56:11Z")
    cc.Store(store_dir).put(sealed)
    before = _stored_bytes(store_dir, sealed)

    restage = cc.Attestation.from_cbor(entry.to_cbor())
    restage.sign(historian, role="historian", signed_at="2026-06-14T09:00:00Z")
    cc.write_pending("1-session-entry", restage, [])

    cc.cmd_finalize(Namespace(force=True, imeanit=False))

    assert _stored_bytes(store_dir, sealed) == before        # untouched
    assert list(pending.glob("*"))                           # kept for retry


# --- guard 3: verify trial-log drift ---------------------------------------------

def test_verify_flags_entry_missing_from_trial_log(trial, capsys):
    cc, base, store_dir, pending = trial
    (base / "trial-log.md").write_text("# Continuity Trial — Log\n\n## Session 0\n")
    session = cc.Steward.generate()
    entry = _entry(cc, session, 3)
    cc.Store(store_dir).put(entry)

    rc = cc.cmd_verify(Namespace())
    out = capsys.readouterr().out
    assert rc == 1
    assert "DRIFT" in out
    assert entry.id in out


def test_verify_clean_when_entry_present_in_log(trial, capsys):
    cc, base, store_dir, pending = trial
    session = cc.Steward.generate()
    entry = _entry(cc, session, 3)
    cc.Store(store_dir).put(entry)
    (base / "trial-log.md").write_text(
        f"# Continuity Trial — Log\n\n## Session 3\n- entry attestation: {entry.id}\n")

    rc = cc.cmd_verify(Namespace())
    out = capsys.readouterr().out
    assert rc == 0
    assert "DRIFT" not in out


# --- guard 2: git durability/drift (needs a real git tree) -----------------------

def _git(repo, *args):
    import subprocess
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True)


@pytest.fixture
def git_trial(cc, tmp_path, monkeypatch):
    import shutil
    if shutil.which("git") is None:
        pytest.skip("git not available")
    base = tmp_path / "continuity"
    store = base / "store"
    pending = base / "pending"
    store.mkdir(parents=True)
    pending.mkdir(parents=True)
    _git(tmp_path, "init", "-q")
    _git(tmp_path, "config", "user.email", "t@example.com")
    _git(tmp_path, "config", "user.name", "t")
    monkeypatch.setattr(cc, "REPO", tmp_path)
    monkeypatch.setattr(cc, "BASE", base)
    monkeypatch.setattr(cc, "STORE", store)
    monkeypatch.setattr(cc, "PENDING", pending)
    return cc, tmp_path, base, store, pending


def test_verify_flags_uncommitted_store_file(git_trial, capsys):
    cc, repo, base, store_dir, pending = git_trial
    (base / "trial-log.md").write_text("# log\n")
    session = cc.Steward.generate()
    entry = _entry(cc, session, 3)
    cc.Store(store_dir).put(entry)
    (base / "trial-log.md").write_text(f"# log\n{entry.id}\n")   # not log-drift

    rc = cc.cmd_verify(Namespace())
    out = capsys.readouterr().out
    assert rc == 1
    assert "sealed but not committed" in out

    _git(repo, "add", "-A")
    _git(repo, "commit", "-q", "-m", "seal")
    rc2 = cc.cmd_verify(Namespace())
    assert rc2 == 0


# --- commit signing: session key <-> openssh, allowed_signers --------------------

def test_session_key_exports_to_loadable_openssh(cc):
    from cryptography.hazmat.primitives.serialization import (
        load_ssh_private_key, Encoding, PublicFormat)
    s = cc.Steward.generate()
    pem = cc.session_key_openssh(s)
    loaded = load_ssh_private_key(pem, password=None)
    raw = loaded.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    assert raw == s.pubkey                       # same key git would sign with


def test_ssh_pubkey_line_roundtrips(cc):
    s = cc.Steward.generate()
    line = cc.ssh_pubkey_line(s.pubkey, comment=s.id)
    assert cc.pub_from_ssh_pubkey(line) == s.pubkey
    assert cc.pub_of_steward_id(s.id) == s.pubkey


def test_allowed_signers_lists_countersigned_keys(trial):
    cc, base, store_dir, pending = trial
    target = cc.Steward.generate()
    historian = cc.Steward.generate()
    endorsement = cc.Attestation.build(
        cc.claims.endorsement(target=target.id, in_capacity="session-instance",
                              rationale="test countersign"),
        community="continuity-trial").sign(historian, role="guardian")
    store = cc.Store(store_dir)
    store.put(endorsement)

    text = cc.allowed_signers_from_store(store)
    assert target.id in text
    assert cc.ssh_pubkey_line(target.pubkey) in text
    # `extra` folds in a live session not yet countersigned in the store
    live = cc.Steward.generate()
    text2 = cc.allowed_signers_from_store(store, extra=[(live.id, live.pubkey)])
    assert live.id in text2 and target.id in text2
