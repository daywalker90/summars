import os
import random
import string
from hashlib import sha256
from pathlib import Path

import pytest
from pyln.testing.utils import TIMEOUT

RUST_PROFILE = os.environ.get("RUST_PROFILE", "debug")
plugin_dir = Path(__file__).parent.parent.resolve()
COMPILED_PATH = plugin_dir / "target" / RUST_PROFILE / "summars"
DOWNLOAD_PATH = plugin_dir / "tests" / "summars"


@pytest.fixture
def get_plugin(directory):
    if COMPILED_PATH.is_file():
        return COMPILED_PATH
    elif DOWNLOAD_PATH.is_file():
        return DOWNLOAD_PATH
    else:
        raise ValueError("No files were found.")


def generate_random_label():
    label_length = 8
    random_label = "".join(
        random.choice(string.ascii_letters) for _ in range(label_length)
    )
    return random_label


def generate_random_number():
    return random.randint(1, 20_000_000_000_000_00_000)


def my_xpay(node, invstring, partial_msat=None):
    params = {"invstring": invstring, "retry_for": TIMEOUT}
    if partial_msat:
        params["partial_msat"] = partial_msat
    return node.rpc.call("xpay", params)


def new_preimage() -> tuple[str, str]:
    preimage = os.urandom(32)
    return preimage.hex(), sha256(preimage).hexdigest()
