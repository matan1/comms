"""Identity handshake: challenge / bound response / verify.

The connection-time rite from `docs/sentira-motes-identity-handshake.1.0.md`:
prove **key possession** and bind a connection's **purpose**, without
asserting any authority to act. It answers exactly one question — "is this
endpoint the same identity as before, and did it mean to talk to me, now,
about this?" — and deliberately nothing else. "Reachable" is not "allowed";
a LAN is location, not authorization.

Shape (mirroring the capability rite, but for identity rather than work):

  1. challenge      -- the attaching side issues a fresh single-use nonce.
  2. bound response -- the responder signs a `session-bind` claim naming the
                       nonce, itself, the expected peer, the stream purpose,
                       and a short expiry. Signed `role: "party"`.
  3. verify         -- the challenger checks the signature, the nonce, the
                       peer binding, and freshness. A reused or stale nonce,
                       or a peer-id mismatch, fails: this defeats replay.
  4. mutual         -- both sides run 1-3, each authenticating the other.
  5. broker record  -- a tracker MAY witness that it arranged the pairing: a
                       custody record, never an authorization.

What a passed handshake is: evidence of key possession and intent, at a
moment, for a purpose. What it is not: actuation authority, delegation, or
a trust decision — those live in the viewer's policy, never here.
"""

from __future__ import annotations

from datetime import datetime, timezone

import cbor2

from .attest import Attestation
from .canonical import canonical_cbor
from .ceremony import new_nonce
from .identity import Steward
from . import claims as C

#: Default bound-response lifetime, seconds. Connection setup is fast; a
#: response that took minutes to arrive deserves suspicion, not service.
DEFAULT_TTL = 120

_BIND_KIND = "session-bind"
_BIND_MEDIA = "application/cbor"
_TS = "%Y-%m-%dT%H:%M:%SZ"


def _now() -> str:
    return datetime.now(timezone.utc).strftime(_TS)


def _parse_ts(ts: str) -> datetime | None:
    try:
        return datetime.strptime(ts, _TS).replace(tzinfo=timezone.utc)
    except (TypeError, ValueError):
        return None


class NonceLedger:
    """Single-use nonce tracking: the mechanism for "fresh" and "spent".

    The substrate provides issuance and single spend; retention, sharing, and
    what to do about strangers' nonces are community policy. One ledger
    belongs to one challenger — nonces are not global.
    """

    def __init__(self):
        self._issued: set[str] = set()
        self._spent: set[str] = set()

    def issue(self) -> str:
        nonce = new_nonce()
        self._issued.add(nonce)
        return nonce

    def spend(self, nonce: str) -> tuple[bool, str]:
        """Consume a nonce. Fails on anything not issued here or already used."""
        if nonce not in self._issued:
            return False, "unknown nonce (not issued by this challenger)"
        if nonce in self._spent:
            return False, "nonce already spent (replay)"
        self._spent.add(nonce)
        return True, "ok"


def bind_response(responder: Steward, *, nonce: str, peer: str, purpose: str,
                  ttl: int = DEFAULT_TTL, expires_at: str | None = None,
                  now: str | None = None) -> Attestation:
    """Step 2: the responder's signed answer to a challenge.

    Binds the nonce, both identities, and the purpose into one signed body,
    so no field can be lifted into a different conversation. `peer` is the
    steward id the responder believes it is talking to; a verifier who is
    not that peer must reject the response.
    """
    issued = now or _now()
    if expires_at is None:
        t = _parse_ts(issued)
        expires_at = datetime.fromtimestamp(
            t.timestamp() + ttl, tz=timezone.utc).strftime(_TS)
    body = {
        "nonce": nonce,
        "responder": responder.id,
        "peer": peer,
        "purpose": purpose,
        "expires_at": expires_at,
    }
    claim = C.general_claim(
        about=f"session-bind {responder.id} -> {peer}",
        kind=_BIND_KIND,
        body=canonical_cbor(body),
        media_type=_BIND_MEDIA,
    )
    att = Attestation.build(claim, occasion="identity handshake")
    return att.sign(responder, role="party")


def verify_bound_response(att: Attestation, *, nonce: str, responder: str,
                          verifier: str, purpose: str | None = None,
                          ledger: NonceLedger | None = None,
                          now: str | None = None) -> tuple[bool, str]:
    """Step 3: the challenger's judgment of a bound response.

    Checks, in order: the envelope verifies (layers 1-2); the claim is a
    session-bind; the body decodes; the nonce matches (and, with a ledger,
    is fresh and single-use — the ledger entry is spent by this call); the
    responder named in the body signed as `party`; the response was bound to
    *this* verifier; it has not expired; and, if asked, the purpose matches.

    Returns (ok, reason). A True result is evidence of key possession and
    intent — never authority, and never a trust decision.
    """
    ok, why = att.verified()
    if not ok:
        return False, why

    claim = att.claim
    if claim.get("t") != "general-claim/1" or claim.get("kind") != _BIND_KIND:
        return False, "not a session-bind claim"
    content = claim.get("content", {})
    if content.get("media_type") != _BIND_MEDIA:
        return False, "session-bind body must be application/cbor"
    try:
        body = cbor2.loads(content.get("body", b""))
    except Exception:
        return False, "session-bind body does not decode"
    if not isinstance(body, dict):
        return False, "session-bind body is not a map"

    if body.get("nonce") != nonce:
        return False, "nonce mismatch"
    if ledger is not None:
        ok, why = ledger.spend(nonce)
        if not ok:
            return False, why

    if body.get("responder") != responder:
        return False, "responder mismatch"
    party_signers = {s["by"] for s in att.signatures if s.get("role") == "party"}
    if responder not in party_signers:
        return False, "responder did not sign as party"

    if body.get("peer") != verifier:
        return False, "response bound to a different peer"

    exp = _parse_ts(body.get("expires_at", ""))
    if exp is None:
        return False, "expiry missing or not canonical"
    at = _parse_ts(now or _now())
    if at > exp:
        return False, "response expired"

    if purpose is not None and body.get("purpose") != purpose:
        return False, "purpose mismatch"

    return True, "ok"


def mutual_handshake(a: Steward, b: Steward, *, purpose: str,
                     ledger_a: NonceLedger | None = None,
                     ledger_b: NonceLedger | None = None) -> dict:
    """Steps 1-4 both ways: each side challenges and verifies the other.

    A convenience driver for tests and single-process use; over a real
    transport the same four calls happen with a wire in between. Returns the
    two bound responses and the overall outcome — evidence, not admission.
    """
    ledger_a = ledger_a or NonceLedger()
    ledger_b = ledger_b or NonceLedger()

    nonce_a = ledger_a.issue()  # a challenges b
    nonce_b = ledger_b.issue()  # b challenges a
    response_b = bind_response(b, nonce=nonce_a, peer=a.id, purpose=purpose)
    response_a = bind_response(a, nonce=nonce_b, peer=b.id, purpose=purpose)

    ok_b, why_b = verify_bound_response(
        response_b, nonce=nonce_a, responder=b.id, verifier=a.id,
        purpose=purpose, ledger=ledger_a)
    ok_a, why_a = verify_bound_response(
        response_a, nonce=nonce_b, responder=a.id, verifier=b.id,
        purpose=purpose, ledger=ledger_b)

    return {
        "ok": ok_a and ok_b,
        "a_verified_b": (ok_b, why_b),
        "b_verified_a": (ok_a, why_a),
        "response_a": response_a,
        "response_b": response_b,
    }


def broker_record(tracker: Steward, response_a: Attestation,
                  response_b: Attestation, note: str | None = None) -> Attestation:
    """Step 5: a tracker's custody witness that it arranged a pairing.

    References both bound responses and is signed in the custodian role. It
    proves *that a pairing was brokered*, never *that the pairing was
    permitted* — brokering is transport, not the conferral of authority.
    """
    claim = C.general_claim(
        about=f"brokered pairing {response_a.id} <-> {response_b.id}",
        kind="brokered-pairing",
        body=note or "pairing arranged; custody record, not authorization",
        support=[response_a.id, response_b.id],
    )
    att = Attestation.build(
        claim,
        occasion="identity handshake",
        refs=[
            {"role": "context", "id": response_a.id},
            {"role": "context", "id": response_b.id},
        ],
    )
    return att.sign(tracker, role="custodian")
