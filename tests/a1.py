"""Independent verifier for Attest 1.0 (as amended by A1) and Steward 1.0.

Implemented from the spec documents (docs/comms.spec.1.0.md,
docs/attest-1.0-amendment-A1.md, docs/comms-steward-1.0-sketch.md), not from
the toolkit or the vector generators, so the golden-vector tests check the
spec against a second implementation as A1.9 requires. Uses cbor2, blake3,
and PyNaCl directly; shares no code with the comms package.
"""

from __future__ import annotations

import cbor2
import blake3
from nacl.signing import SigningKey, VerifyKey
from nacl.exceptions import BadSignatureError

CTX_CORE = b"comms.attest.core/1"
CTX_KEYSET = b"comms.keyset/1"
CTX_BUNDLE = b"comms.bundle/1"

_B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def b58encode(data: bytes) -> str:
    n = int.from_bytes(data, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = _B58[r] + out
    pad = len(data) - len(data.lstrip(b"\x00"))
    return "1" * pad + out


def b58decode(s: str) -> bytes:
    n = 0
    for ch in s:
        n = n * 58 + _B58.index(ch)
    body = n.to_bytes((n.bit_length() + 7) // 8, "big") if n else b""
    pad = len(s) - len(s.lstrip("1"))
    return b"\x00" * pad + body


def multibase_z(data: bytes) -> str:
    return "z" + b58encode(data)


def canon(obj) -> bytes:
    """RFC 8949 §4.2.1 core deterministic encoding."""
    return cbor2.dumps(obj, canonical=True)


def dsh(ctx: bytes, data: bytes) -> bytes:
    """A1.1: H(ctx, D) = blake3(uint8(len(ctx)) || ctx || D)."""
    assert 0 < len(ctx) < 256
    return blake3.blake3(bytes([len(ctx)]) + ctx + data).digest()


def attest_id(core: dict) -> str:
    """A1.2."""
    return "comms.attest:" + multibase_z(dsh(CTX_CORE, canon(core)))


def personal_steward_id(pub: bytes) -> str:
    return "comms.steward:" + multibase_z(pub)


def pub_from_steward_id(steward_id: str) -> bytes:
    assert steward_id.startswith("comms.steward:z")
    return b58decode(steward_id[len("comms.steward:z"):])


# ---- A1.3 signatures -----------------------------------------------------------

def sig_payload(core: dict, sig_obj: dict) -> bytes:
    """Reconstruct the signed payload from the signature object's own fields
    plus the locally computed core hash (A1.3). The ed25519-set/1 variant
    additionally binds the keyset attestation id (Steward 1.0 §3.2)."""
    payload = {
        "t": "comms.sig/1",
        "core": dsh(CTX_CORE, canon(core)),
        "by": sig_obj["by"],
        "alg": sig_obj["alg"],
        "role": sig_obj["role"],
        "signed_at": sig_obj["signed_at"],
    }
    if sig_obj["alg"] == "ed25519-set/1":
        payload["keyset"] = sig_obj["keyset"]
    return canon(payload)


def verify_personal_signature(core: dict, sig_obj: dict) -> bool:
    if sig_obj.get("alg") != "ed25519":
        return False
    pub = pub_from_steward_id(sig_obj["by"])
    try:
        VerifyKey(pub).verify(sig_payload(core, sig_obj), sig_obj["signature"])
        return True
    except BadSignatureError:
        return False


def sign_personal(core: dict, sk: SigningKey, *, role: str,
                  signed_at: str) -> dict:
    pub = sk.verify_key.encode()
    obj = {"by": personal_steward_id(pub), "alg": "ed25519",
           "signed_at": signed_at, "role": role}
    obj["signature"] = sk.sign(sig_payload(core, obj)).signature
    return obj


# ---- Steward 1.0 ----------------------------------------------------------------

def keyset_descriptor(member_keys: list[bytes], threshold: int) -> dict:
    """§1: members sorted by bytewise key comparison, unique, flat n-of-m."""
    assert 1 <= threshold <= len(member_keys)
    assert len(set(member_keys)) == len(member_keys)
    return {"v": 1,
            "members": sorted(({"key": k} for k in member_keys),
                              key=lambda m: m["key"]),
            "threshold": threshold}


def community_id(genesis_descriptor: dict) -> str:
    """§2: genesis-anchored, never changes across rotation."""
    return "comms.steward:" + multibase_z(
        dsh(CTX_KEYSET, canon(genesis_descriptor)))


def sign_set(core: dict, *, by: str, role: str, signed_at: str,
             keyset_attest_id: str, signers: list[SigningKey]) -> dict:
    """§3.2: one signature object whose bytes are a canonical CBOR array of
    {k, s} inner signatures sorted by k."""
    obj = {"by": by, "alg": "ed25519-set/1", "signed_at": signed_at,
           "role": role, "keyset": keyset_attest_id}
    payload = sig_payload(core, obj)
    inner = sorted(
        ({"k": sk.verify_key.encode(), "s": sk.sign(payload).signature}
         for sk in signers),
        key=lambda e: e["k"])
    obj["signature"] = canon(inner)
    return obj


def verify_set_signature(core: dict, sig_obj: dict, descriptor: dict) -> bool:
    """§3.2 verification: >= threshold distinct descriptor keys with valid
    signatures; non-member inner signatures are ignored (pad, not poison);
    duplicate inner keys reject; an invalid signature from a member rejects."""
    if sig_obj.get("alg") != "ed25519-set/1":
        return False
    payload = sig_payload(core, sig_obj)
    inner = cbor2.loads(sig_obj["signature"])
    keys = [e["k"] for e in inner]
    if len(keys) != len(set(keys)):
        return False
    members = {m["key"] for m in descriptor["members"]}
    valid = 0
    for e in inner:
        if e["k"] not in members:
            continue
        try:
            VerifyKey(e["k"]).verify(payload, e["s"])
        except BadSignatureError:
            return False
        valid += 1
    return valid >= descriptor["threshold"]


def verify_chain(community: str, keyset_attest_id: str, store: dict) -> dict:
    """§4: walk a keyset/1 attestation back to genesis; return the descriptor
    it establishes or raise ValueError. `store` maps attestation id to
    {"core": ..., "signatures": [...]}; verification is offline."""
    att = store[keyset_attest_id]
    desc = att["core"]["c"]["descriptor"]
    supersedes = [r["id"] for r in att["core"].get("r", [])
                  if r["role"] == "supersedes"]
    sig = next(s for s in att["signatures"]
               if s["alg"] == "ed25519-set/1" and s["by"] == community)
    if not supersedes:
        # genesis: descriptor must hash to the community id, and the link must
        # be threshold-signed under its own descriptor, referencing itself
        if community_id(desc) != community:
            raise ValueError("genesis descriptor does not hash to community id")
        if sig["keyset"] != keyset_attest_id:
            raise ValueError("genesis signature must reference itself")
        if not verify_set_signature(att["core"], sig, desc):
            raise ValueError("genesis not threshold-signed under own descriptor")
        return desc
    if len(supersedes) != 1:
        raise ValueError("exactly one supersedes ref required per link")
    prev_desc = verify_chain(community, supersedes[0], store)
    if sig["keyset"] != supersedes[0]:
        raise ValueError("rotation must be authorized by predecessor keyset")
    if not verify_set_signature(att["core"], sig, prev_desc):
        raise ValueError("rotation not threshold-signed by predecessor")
    return desc


def verify_community_attestation(att: dict, community: str, store: dict) -> bool:
    sig = next(s for s in att["signatures"]
               if s["alg"] == "ed25519-set/1" and s["by"] == community)
    try:
        desc = verify_chain(community, sig["keyset"], store)
    except ValueError:
        return False
    return verify_set_signature(att["core"], sig, desc)
