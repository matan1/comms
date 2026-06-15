import json
from pathlib import Path

import comms


def test_signed_reference_policy_vector():
    vector = json.loads(
        (Path(__file__).parents[1] / "data/vouch-reference-policy.json").read_text()
    )
    att = comms.Attestation.from_cbor(bytes.fromhex(vector["canonical_cbor_hex"]))
    assert att.id == vector["attestation_id"]
    assert att.claim == vector["claim"]
    assert att.signatures_valid()
    assert att.signers() == {vector["author_steward_id"]}
