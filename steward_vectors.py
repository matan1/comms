"""Steward layer reference implementation + golden vectors.

Implements the Steward 1.0 sketch:
  - keyset descriptors (flat n-of-m, plain Ed25519 member keys)
  - genesis-anchored community identity: ID = H("comms.keyset/1", canon(genesis descriptor))
  - community signatures: alg "ed25519-set/1", one signature object whose bytes are a
    canonical CBOR array of {k: member pubkey, s: plain Ed25519 sig} over a payload
    that includes the keyset attestation ID it claims validity under
  - keyset rotation via supersedes chain, each link authorized by its predecessor
  - full offline chain verification back to genesis
  - succession claims for broken chains, witnessed by surviving member keys
Negative vectors prove sub-threshold and wrong-keyset signatures fail.
"""
import json
import cbor2
import blake3
from nacl.signing import SigningKey, VerifyKey
from nacl.exceptions import BadSignatureError

# ---- shared primitives (identical to gen_vectors.py / Amendment A1) ----------
B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

def b58encode(b: bytes) -> str:
    n = int.from_bytes(b, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = B58[r] + out
    pad = 0
    for byte in b:
        if byte == 0:
            pad += 1
        else:
            break
    return "1" * pad + out

def mb(b: bytes) -> str:
    return "z" + b58encode(b)

CTX_CORE = b"comms.attest.core/1"
CTX_KEYSET = b"comms.keyset/1"

def dsh(ctx: bytes, data: bytes) -> bytes:
    assert len(ctx) < 256
    return blake3.blake3(bytes([len(ctx)]) + ctx + data).digest()

def canon(obj) -> bytes:
    return cbor2.dumps(obj, canonical=True)

def attest_id(core: dict) -> str:
    return "comms.attest:" + mb(dsh(CTX_CORE, canon(core)))

# ---- steward layer ------------------------------------------------------------
def keyset_descriptor(member_keys, threshold):
    """Descriptor: {v, members (sorted by key bytes), threshold}. Members are
    bare Ed25519 public keys; names/bindings live in ceremony records, not here."""
    assert 1 <= threshold <= len(member_keys)
    members = sorted(({"key": k} for k in member_keys), key=lambda m: m["key"])
    return {"v": 1, "members": members, "threshold": threshold}

def community_id(genesis_descriptor) -> str:
    return "comms.steward:" + mb(dsh(CTX_KEYSET, canon(genesis_descriptor)))

def personal_id(pub: bytes) -> str:
    return "comms.steward:" + mb(pub)

def set_payload(core_hash, by, role, signed_at, keyset_attest_id):
    return canon({"t": "comms.sig/1", "core": core_hash, "by": by,
                  "alg": "ed25519-set/1", "role": role,
                  "signed_at": signed_at, "keyset": keyset_attest_id})

def community_sign(core: dict, by, role, signed_at, keyset_attest_id, signers):
    """signers: list of (SigningKey, pubkey). Returns a signature object."""
    payload = set_payload(dsh(CTX_CORE, canon(core)), by, role, signed_at, keyset_attest_id)
    inner = sorted(
        ({"k": pub, "s": sk.sign(payload).signature} for sk, pub in signers),
        key=lambda e: e["k"])
    keys = [e["k"] for e in inner]
    assert len(keys) == len(set(keys)), "duplicate member key"
    return {"by": by, "alg": "ed25519-set/1", "signed_at": signed_at,
            "role": role, "keyset": keyset_attest_id, "signature": canon(inner)}

def personal_sign(core: dict, sk: SigningKey, pub: bytes, role, signed_at):
    payload = canon({"t": "comms.sig/1", "core": dsh(CTX_CORE, canon(core)),
                     "by": personal_id(pub), "alg": "ed25519", "role": role,
                     "signed_at": signed_at})
    return {"by": personal_id(pub), "alg": "ed25519", "signed_at": signed_at,
            "role": role, "signature": sk.sign(payload).signature}

def verify_set_signature(core: dict, sig_obj: dict, descriptor: dict) -> bool:
    """Verify one ed25519-set/1 signature object against a specific descriptor.
    Tolerant counting: inner signatures by keys absent from the descriptor are
    ignored (a hostile relay can pad but not poison); duplicate keys reject;
    validity = >= threshold distinct descriptor keys with valid signatures."""
    payload = set_payload(dsh(CTX_CORE, canon(core)), sig_obj["by"], sig_obj["role"],
                          sig_obj["signed_at"], sig_obj["keyset"])
    inner = cbor2.loads(sig_obj["signature"])
    keys = [e["k"] for e in inner]
    if len(keys) != len(set(keys)):
        return False
    member_keys = {m["key"] for m in descriptor["members"]}
    valid = 0
    for e in inner:
        if e["k"] not in member_keys:
            continue
        try:
            VerifyKey(e["k"]).verify(payload, e["s"])
            valid += 1
        except BadSignatureError:
            return False  # a forged sig from a listed key is rejection, not noise
    return valid >= descriptor["threshold"]

def verify_chain(community, keyset_attest_id, store):
    """Walk a keyset attestation back to genesis. Returns the descriptor that
    keyset_attest_id establishes, or raises. `store` maps attestation id ->
    {core, signatures}. Offline: needs only the store contents."""
    att = store[keyset_attest_id]
    desc = att["core"]["c"]["descriptor"]
    refs = att["core"].get("r", [])
    prev = [r["id"] for r in refs if r["role"] == "supersedes"]
    if not prev:
        # genesis: self-certifying (descriptor hash == community id) AND
        # community-signed under its own descriptor (proves key possession)
        if community_id(desc) != community:
            raise ValueError("genesis descriptor does not hash to community id")
        sig = next(s for s in att["signatures"]
                   if s["alg"] == "ed25519-set/1" and s["by"] == community)
        if sig["keyset"] != keyset_attest_id:
            raise ValueError("genesis signature must reference itself")
        if not verify_set_signature(att["core"], sig, desc):
            raise ValueError("genesis not threshold-signed under own descriptor")
        return desc
    if len(prev) != 1:
        raise ValueError("fork or malformed: exactly one supersedes ref required per link")
    prev_desc = verify_chain(community, prev[0], store)
    sig = next(s for s in att["signatures"]
               if s["alg"] == "ed25519-set/1" and s["by"] == community)
    if sig["keyset"] != prev[0]:
        raise ValueError("rotation must be authorized by predecessor keyset")
    if not verify_set_signature(att["core"], sig, prev_desc):
        raise ValueError("rotation not threshold-signed by predecessor")
    return desc

def verify_community_attestation(att, community, store):
    """Full verification of a community-signed attestation, offline."""
    sig = next(s for s in att["signatures"]
               if s["alg"] == "ed25519-set/1" and s["by"] == community)
    desc = verify_chain(community, sig["keyset"], store)
    return verify_set_signature(att["core"], sig, desc)

# ---- build the vectors ----------------------------------------------------------
def kp(seed_byte):
    sk = SigningKey(bytes([seed_byte]) * 32)
    return sk, sk.verify_key.encode()

# founders Ada(0x11), Bea(0x12), Cy(0x13); later member Dov(0x14);
# successor-era members Eve(0x21), Fen(0x22)
ada, bea, cy, dov, eve, fen = (kp(b) for b in (0x11, 0x12, 0x13, 0x14, 0x21, 0x22))

T0, T1, T2, T3, T4 = ("2026-06-11T10:00:00Z", "2026-06-11T10:00:01Z",
                      "2026-07-01T09:00:00Z", "2026-07-01T09:00:01Z",
                      "2027-01-05T12:00:00Z")

# K0: genesis, 2-of-3 {Ada, Bea, Cy}
desc0 = keyset_descriptor([ada[1], bea[1], cy[1]], 2)
CID = community_id(desc0)

k0_core = {"v": 1, "t": "comms.attestation/1",
           "c": {"t": "keyset/1", "community": CID, "descriptor": desc0},
           "f": {"issued_at": T0, "language": "en",
                 "occasion": "founding of the Northfield commons"},
           "r": []}
K0 = attest_id(k0_core)
k0_att = {"core": k0_core,
          "signatures": [community_sign(k0_core, CID, "community", T1, K0,
                                        [(ada[0], ada[1]), (bea[0], bea[1])])]}

# rule/1 under K0
rule_core = {"v": 1, "t": "comms.attestation/1",
             "c": {"t": "rule/1", "community_name": "Northfield Commons", "community": CID,
                   "document": {"media_type": "text/markdown",
                                "body": b"# Northfield Rule v1\nSuccession requires two founding-member witnesses and endorsement by the Eastbrook community."}},
             "f": {"issued_at": T0, "language": "en"}, "r": []}
RULE = attest_id(rule_core)
rule_att = {"core": rule_core,
            "signatures": [community_sign(rule_core, CID, "community", T1, K0,
                                          [(bea[0], bea[1]), (cy[0], cy[1])])]}

# K1: rotation — Cy leaves, Dov joins; authorized by 2 of K0
desc1 = keyset_descriptor([ada[1], bea[1], dov[1]], 2)
k1_core = {"v": 1, "t": "comms.attestation/1",
           "c": {"t": "keyset/1", "community": CID, "descriptor": desc1},
           "f": {"issued_at": T2, "language": "en", "occasion": "key rotation: Cy departs, Dov joins"},
           "r": [{"role": "supersedes", "id": K0}]}
K1 = attest_id(k1_core)
k1_att = {"core": k1_core,
          "signatures": [community_sign(k1_core, CID, "community", T3, K0,
                                        [(ada[0], ada[1]), (cy[0], cy[1])])]}

# rule amendment under K1 — verifies through the chain even though Cy is gone
rule2_core = {"v": 1, "t": "comms.attestation/1",
              "c": {"t": "rule/1", "community_name": "Northfield Commons", "community": CID,
                    "document": {"media_type": "text/markdown",
                                 "body": b"# Northfield Rule v2\nAmended quorum for seasonal ceremonies."},
                    "amendment_summary": "seasonal quorum change"},
              "f": {"issued_at": T2, "language": "en"},
              "r": [{"role": "supersedes", "id": RULE}]}
RULE2 = attest_id(rule2_core)
rule2_att = {"core": rule2_core,
             "signatures": [community_sign(rule2_core, CID, "community", T3, K1,
                                           [(bea[0], bea[1]), (dov[0], dov[1])])]}

store = {K0: k0_att, K1: k1_att}

# ---- positive checks -------------------------------------------------------------
assert verify_community_attestation(rule_att, CID, store), "rule under K0"
assert verify_community_attestation(rule2_att, CID, store), "rule2 under K1 via chain"
assert verify_chain(CID, K1, store) == desc1

# ---- negative checks --------------------------------------------------------------
neg = {}

# N1: sub-threshold (1-of-2 required signatures)
bad1 = dict(rule2_att)
bad1["signatures"] = [community_sign(rule2_core, CID, "community", T3, K1,
                                     [(bea[0], bea[1])])]
neg["sub_threshold"] = not verify_community_attestation(bad1, CID, store)
assert neg["sub_threshold"]

# N2: departed member signing under K1 (Cy not in desc1; Bea alone is below threshold)
bad2 = dict(rule2_att)
bad2["signatures"] = [community_sign(rule2_core, CID, "community", T3, K1,
                                     [(cy[0], cy[1]), (bea[0], bea[1])])]
neg["departed_key_ignored"] = not verify_community_attestation(bad2, CID, store)
assert neg["departed_key_ignored"]

# N3: rotation not authorized by predecessor — forged K1' signed only by its own new keys
descX = keyset_descriptor([eve[1], fen[1]], 2)
kx_core = {"v": 1, "t": "comms.attestation/1",
           "c": {"t": "keyset/1", "community": CID, "descriptor": descX},
           "f": {"issued_at": T2, "language": "en"},
           "r": [{"role": "supersedes", "id": K0}]}
KX = attest_id(kx_core)
kx_att = {"core": kx_core,
          "signatures": [community_sign(kx_core, CID, "community", T3, KX,
                                        [(eve[0], eve[1]), (fen[0], fen[1])])]}
storeX = dict(store); storeX[KX] = kx_att
try:
    verify_chain(CID, KX, storeX)
    neg["hostile_takeover_rejected"] = False
except ValueError:
    neg["hostile_takeover_rejected"] = True
assert neg["hostile_takeover_rejected"]

# ---- succession after a broken chain -----------------------------------------------
# Disaster: Ada's and Dov's keys are lost; Bea alone is below K1's threshold.
# Bea + new members Eve, Fen found a NEW genesis and claim succession; Bea and Cy
# (departed founder, key intact) witness with their personal keys.
desc_succ = keyset_descriptor([bea[1], eve[1], fen[1]], 2)
CID2 = community_id(desc_succ)
ks_core = {"v": 1, "t": "comms.attestation/1",
           "c": {"t": "keyset/1", "community": CID2, "descriptor": desc_succ},
           "f": {"issued_at": T4, "language": "en", "occasion": "re-founding after key loss"},
           "r": []}
KS = attest_id(ks_core)
ks_att = {"core": ks_core,
          "signatures": [community_sign(ks_core, CID2, "community", T4, KS,
                                        [(bea[0], bea[1]), (eve[0], eve[1])])]}

succ_core = {"v": 1, "t": "comms.attestation/1",
             "c": {"t": "succession/1", "predecessor": CID, "successor": CID2,
                   "account": "Two of three K1 keys were lost in the December flood; the community re-keyed in assembly.",
                   "continuity_basis": RULE},
             "f": {"issued_at": T4, "language": "en"},
             "r": [{"role": "successor-of", "id": K1},
                   {"role": "context", "id": RULE}]}
SUCC = attest_id(succ_core)
succ_att = {"core": succ_core, "signatures": [
    community_sign(succ_core, CID2, "author", T4, KS, [(bea[0], bea[1]), (fen[0], fen[1])]),
    personal_sign(succ_core, bea[0], bea[1], "witness", T4),
    personal_sign(succ_core, cy[0], cy[1], "witness", T4),
]}
store2 = dict(store); store2[KS] = ks_att
assert verify_community_attestation(succ_att, CID2, store2)
# The succession claim is VALID and VERIFIED but its authority is a trust
# judgment: the witness signatures by Bea (current member of old K1) and Cy
# (founding member, departed but key intact) are the evidence, checked against RULE's continuity clause.
def verify_personal(core, s, pub):
    payload = canon({"t": "comms.sig/1", "core": dsh(CTX_CORE, canon(core)),
                     "by": s["by"], "alg": "ed25519", "role": s["role"],
                     "signed_at": s["signed_at"]})
    VerifyKey(pub).verify(payload, s["signature"])

verify_personal(succ_core, succ_att["signatures"][1], bea[1])
verify_personal(succ_core, succ_att["signatures"][2], cy[1])

# ---- emit ---------------------------------------------------------------------------
def sig_out(s):
    o = {k: (v.hex() if isinstance(v, bytes) else v) for k, v in s.items()}
    return o

def att_out(name, att, aid):
    return {"name": name, "attestation_id": aid,
            "canonical_core_cbor_hex": canon(att["core"]).hex(),
            "core_hash_hex": dsh(CTX_CORE, canon(att["core"])).hex(),
            "signatures": [sig_out(s) for s in att["signatures"]]}

vectors = {
    "scheme": {
        "descriptor": "{v, members:[{key}], threshold}; members sorted by key bytes",
        "community_id": "comms.steward:z + multibase(H('comms.keyset/1', canonical_cbor(genesis descriptor)))",
        "set_signature": "alg ed25519-set/1; signature bytes = canonical CBOR array of {k: pubkey, s: pure-Ed25519 sig} sorted by k; each member signs the canonical sig payload {t,core,by,alg,role,signed_at,keyset}",
        "chain_rule": "each keyset/1 link carries one supersedes ref and a set signature whose keyset field names the predecessor and which meets the predecessor's threshold; genesis self-certifies (descriptor hash == community id) and self-signs",
    },
    "keys": [{"name": n, "ed25519_seed_hex": s, "public_key_hex": p.hex(),
              "personal_steward_id": personal_id(p)}
             for n, s, p in [("Ada", "11"*32, ada[1]), ("Bea", "12"*32, bea[1]),
                             ("Cy", "13"*32, cy[1]), ("Dov", "14"*32, dov[1]),
                             ("Eve", "21"*32, eve[1]), ("Fen", "22"*32, fen[1])]],
    "community_id": CID,
    "successor_community_id": CID2,
    "vectors": [
        att_out("K0 genesis keyset (2-of-3 Ada/Bea/Cy), self-signed", k0_att, K0),
        att_out("rule/1 community-signed under K0", rule_att, RULE),
        att_out("K1 rotation (Cy out, Dov in), authorized by K0", k1_att, K1),
        att_out("rule/1 amendment community-signed under K1 (verifies through chain)", rule2_att, RULE2),
        att_out("successor genesis KS after key loss", ks_att, KS),
        att_out("succession/1 claim, community-signed by successor + personal witness sigs from Bea and Cy", succ_att, SUCC),
    ],
    "negative_vectors": [
        {"name": "sub-threshold set signature must fail (1 valid sig, threshold 2)", "verified_fails": neg["sub_threshold"]},
        {"name": "departed member's key ignored under K1; remaining sigs below threshold must fail", "verified_fails": neg["departed_key_ignored"]},
        {"name": "hostile takeover: keyset claiming to supersede K0 without predecessor authorization must fail chain verification", "verified_fails": neg["hostile_takeover_rejected"]},
    ],
}

with open("/home/claude/steward-test-vectors.json", "w") as f:
    json.dump(vectors, f, indent=2)

print("community id:", CID)
print("successor id:", CID2)
print("chain K0->K1 verifies; rules verify under both; all negatives fail correctly")
