"""The vector generators must reproduce the committed golden vectors
byte-for-byte. This is the determinism lock: if encoding, hashing, or signing
drifts anywhere in the dependency stack, these fail first.
"""

import subprocess
import sys


def run(script, repo_root, out_dir):
    subprocess.run([sys.executable, str(script), str(out_dir)],
                   cwd=repo_root, check=True, capture_output=True)


def test_attest_vectors_regenerate_identically(repo_root, tmp_path):
    run(repo_root / "scripts" / "gen_vectors.py", repo_root, tmp_path)
    fresh = (tmp_path / "attest-1.0-test-vectors.json").read_bytes()
    committed = (repo_root / "data" / "attest-1.0-test-vectors.json").read_bytes()
    assert fresh == committed


def test_steward_vectors_regenerate_identically(repo_root, tmp_path):
    run(repo_root / "comms" / "steward_vectors.py", repo_root, tmp_path)
    fresh = (tmp_path / "steward-test-vectors.json").read_bytes()
    committed = (repo_root / "data" / "steward-test-vectors.json").read_bytes()
    assert fresh == committed
