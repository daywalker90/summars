import string
import random
import logging
import os
import pytest
from pathlib import Path
from pyln.testing.utils import TIMEOUT

RUST_PROFILE = os.environ.get("RUST_PROFILE", "debug")
COMPILED_PATH = Path.cwd() / "target" / RUST_PROFILE / "summars"
DOWNLOAD_PATH = Path.cwd() / "tests" / "summars"


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
    LOGGER = logging.getLogger(__name__)
    try:
        if partial_msat:
            node.rpc.call(
                "xpay",
                {
                    "invstring": invstring,
                    "retry_for": TIMEOUT,
                    "partial_msat": partial_msat,
                },
            )
        else:
            node.rpc.call(
                "xpay",
                {
                    "invstring": invstring,
                    "retry_for": TIMEOUT,
                },
            )
    except Exception as e:
        LOGGER.info(f"Error paying payment hash:{e}")
        pass
