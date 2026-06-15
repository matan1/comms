import json
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent

# `import comms` resolves to the subpackage at REPO_ROOT/comms/, so REPO_ROOT
# itself goes on the path. (REPO_ROOT.parent is kept for back-compat with the
# old layout, where the checkout dir itself was the package; harmless now.)
sys.path.insert(0, str(REPO_ROOT.parent))
sys.path.insert(0, str(REPO_ROOT))
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
