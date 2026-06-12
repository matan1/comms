import json
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent

# `import comms` resolves to this checkout (the repo root is the package dir,
# so its parent goes on the path; the checkout must be named "comms").
assert REPO_ROOT.name == "comms", "checkout must be named 'comms' to import"
sys.path.insert(0, str(REPO_ROOT.parent))
sys.path.insert(0, str(REPO_ROOT / "tests"))


@pytest.fixture(scope="session")
def repo_root() -> Path:
    return REPO_ROOT


@pytest.fixture(scope="session")
def attest_vectors() -> dict:
    return json.loads((REPO_ROOT / "data" / "attest-1.0-test-vectors.json").read_text())


@pytest.fixture(scope="session")
def steward_vectors() -> dict:
    return json.loads((REPO_ROOT / "data" / "steward-test-vectors.json").read_text())
