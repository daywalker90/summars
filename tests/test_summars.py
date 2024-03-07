#!/usr/bin/python

import pytest
from pyln.client import RpcError
from pyln.testing.fixtures import *  # noqa: F403
from pyln.testing.utils import sync_blockheight
from util import get_plugin  # noqa: F401

columns = [
    "GRAPH_SATS",
    "OUT_SATS",
    "IN_SATS",
    "SCID",
    "MAX_HTLC",
    "FLAG",
    "BASE",
    "PPM",
    "ALIAS",
    "PEER_ID",
    "UPTIME",
    "HTLCS",
    "STATE",
]


def test_basic(node_factory, get_plugin):  # noqa: F811
    node = node_factory.get_node(options={"plugin": get_plugin})
    result = node.rpc.call("summars", {"summars-locale": "en_US"})
    assert result is not None
    assert isinstance(result, dict) is True
    assert "result" in result
    assert "address" in result["result"]
    assert "forwards" not in result["result"]
    assert "pays" not in result["result"]
    assert "invoices" not in result["result"]

    assert "utxo_amount=0.00000000 BTC" in result["result"]
    assert "avail_out=0.00000000 BTC" in result["result"]
    assert "avail_in=0.00000000 BTC" in result["result"]

    expected_columns = [x for x in columns if x != "GRAPH_SATS"]
    for column in expected_columns:
        assert column in result["result"]

    unexpected_columns = ["GRAPH_SATS"]
    for column in unexpected_columns:
        assert column not in result["result"]

    with pytest.raises(
        RpcError, match="summars-columns must be " "a comma separated string"
    ):
        node.rpc.call("summars", {"summars-columns": 1})

    with pytest.raises(
        RpcError, match="`TEST` not found in " "valid column names"
    ):
        node.rpc.call("summars", {"summars-columns": "TEST"})

    result = node.rpc.call("summars", {"summars-columns": "UPTIME"})
    expected_columns = ["UPTIME"]
    for column in expected_columns:
        assert column in result["result"]

    unexpected_columns = [x for x in columns if x != "UPTIME"]
    for column in unexpected_columns:
        assert column not in result["result"]

    result = node.rpc.call("summars", {"summars-forwards": 1})
    assert "forwards" in result["result"]

    result = node.rpc.call("summars", {"summars-pays": 1})
    assert "pays" in result["result"]

    result = node.rpc.call("summars", {"summars-invoices": 1})
    assert "invoices" in result["result"]

    result = node.rpc.call("summars", {"summars-locale": "de"})
    assert "utxo_amount=0,00000000 BTC" in result["result"]
    assert "avail_out=0,00000000 BTC" in result["result"]
    assert "avail_in=0,00000000 BTC" in result["result"]


def test_options(node_factory, get_plugin):  # noqa: F811
    node = node_factory.get_node(options={"plugin": get_plugin})

    for col in columns:
        result = node.rpc.call("summars", {"summars-columns": col})
        assert col in result["result"]

    for col in columns:
        result = node.rpc.call(
            "summars",
            {"summars-columns": ",".join(columns), "summars-sort-by": col},
        )
        assert col in result["result"]

    result = node.rpc.call("summars", {"summars-exclude-states": "OK"})

    result = node.rpc.call("summars", {"summars-forwards": 1})
    assert "forwards" in result["result"]

    result = node.rpc.call(
        "summars",
        {"summars-forwards": 1, "summars-forwards-filter-amount-msat": 1},
    )
    assert "forwards" in result["result"]

    result = node.rpc.call(
        "summars",
        {"summars-forwards": 1, "summars-forwards-filter-fee-msat": 1},
    )
    assert "forwards" in result["result"]

    result = node.rpc.call(
        "summars", {"summars-forwards": 1, "summars-forwards-alias": False}
    )
    assert "forwards" in result["result"]

    result = node.rpc.call("summars", {"summars-pays": 1})
    assert "pays" in result["result"]

    result = node.rpc.call("summars", {"summars-invoices": 1})
    assert "invoices" in result["result"]

    result = node.rpc.call(
        "summars",
        {"summars-invoices": 1, "summars-invoices-filter-amount-msat": 1},
    )
    assert "invoices" in result["result"]

    result = node.rpc.call("summars", {"summars-locale": "de"})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-refresh-alias": 1})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-max-alias-length": 5})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-availability-interval": 1})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-availability-window": 1})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-utf8": False})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-style": "modern"})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-flow-style": "modern"})
    assert "result" in result


def test_option_errors(node_factory, get_plugin):  # noqa: F811
    node = node_factory.get_node(options={"plugin": get_plugin})
    with pytest.raises(RpcError, match="Not a string"):
        node.rpc.call("summars", {"summars-sort-by": 1})
    with pytest.raises(RpcError, match="Not a valid column name"):
        node.rpc.call("summars", {"summars-sort-by": "TEST"})

    with pytest.raises(RpcError, match="must be a positive number"):
        node.rpc.call("summars", {"summars-forwards": "TEST"})
    with pytest.raises(RpcError, match="Could not read a positive number"):
        node.rpc.call("summars", {"summars-forwards": -1})

    with pytest.raises(RpcError, match="must be a number"):
        node.rpc.call(
            "summars", {"summars-forwards-filter-amount-msat": "TEST"}
        )

    with pytest.raises(RpcError, match="must be a number"):
        node.rpc.call("summars", {"summars-forwards-filter-fee-msat": "TEST"})

    with pytest.raises(RpcError, match="needs to be bool"):
        node.rpc.call("summars", {"summars-forwards-alias": "TEST"})
    with pytest.raises(RpcError, match="needs to be bool"):
        node.rpc.call("summars", {"summars-forwards-alias": 1})

    with pytest.raises(RpcError, match="must be a positive number"):
        node.rpc.call("summars", {"summars-pays": "TEST"})
    with pytest.raises(RpcError, match="Could not read a positive number"):
        node.rpc.call("summars", {"summars-pays": -1})

    with pytest.raises(RpcError, match="must be a positive number"):
        node.rpc.call("summars", {"summars-invoices": "TEST"})
    with pytest.raises(RpcError, match="Could not read a positive number"):
        node.rpc.call("summars", {"summars-invoices": -1})

    with pytest.raises(RpcError, match="must be a number"):
        node.rpc.call(
            "summars", {"summars-invoices-filter-amount-msat": "TEST"}
        )

    with pytest.raises(RpcError, match="Not a valid string"):
        node.rpc.call("summars", {"summars-locale": -1})
    with pytest.raises(RpcError, match="Not a valid locale"):
        node.rpc.call("summars", {"summars-locale": "xxxx"})

    with pytest.raises(RpcError, match="must be a positive number"):
        node.rpc.call("summars", {"summars-refresh-alias": "TEST"})
    with pytest.raises(RpcError, match="Could not read a positive number"):
        node.rpc.call("summars", {"summars-refresh-alias": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to"):
        node.rpc.call("summars", {"summars-refresh-alias": 0})

    with pytest.raises(RpcError, match="must be a positive number"):
        node.rpc.call("summars", {"summars-max-alias-length": "TEST"})
    with pytest.raises(RpcError, match="Could not read a positive number"):
        node.rpc.call("summars", {"summars-max-alias-length": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to"):
        node.rpc.call("summars", {"summars-max-alias-length": 4})

    with pytest.raises(RpcError, match="must be a positive number"):
        node.rpc.call("summars", {"summars-availability-interval": "TEST"})
    with pytest.raises(RpcError, match="Could not read a positive number"):
        node.rpc.call("summars", {"summars-availability-interval": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to"):
        node.rpc.call("summars", {"summars-availability-interval": 0})

    with pytest.raises(RpcError, match="must be a positive number"):
        node.rpc.call("summars", {"summars-availability-window": "TEST"})
    with pytest.raises(RpcError, match="Could not read a positive number"):
        node.rpc.call("summars", {"summars-availability-window": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to"):
        node.rpc.call("summars", {"summars-availability-window": 0})

    with pytest.raises(RpcError, match="needs to be bool"):
        node.rpc.call("summars", {"summars-utf8": "TEST"})
    with pytest.raises(RpcError, match="needs to be bool"):
        node.rpc.call("summars", {"summars-utf8": 1})

    with pytest.raises(RpcError, match="Not a valid string"):
        node.rpc.call("summars", {"summars-style": 1})
    with pytest.raises(RpcError, match="could not parse Style"):
        node.rpc.call("summars", {"summars-style": "TEST"})

    with pytest.raises(RpcError, match="Not a valid string"):
        node.rpc.call("summars", {"summars-flow-style": 1})
    with pytest.raises(RpcError, match="could not parse Style"):
        node.rpc.call("summars", {"summars-flow-style": "TEST"})


def test_chanstates(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2, l3 = node_factory.get_nodes(3, opts={"plugin": get_plugin})
    l1.fundwallet(10_000_000)
    l2.fundwallet(10_000_000)
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    l2.rpc.connect(l3.info["id"], "localhost", l3.port)
    l1.rpc.fundchannel(l2.info["id"], 1_000_000, mindepth=1)
    l2.rpc.fundchannel(l3.info["id"], 1_000_000, mindepth=1)

    result = l2.rpc.call("summars")
    assert l1.info["id"] in result["result"]
    assert l3.info["id"] in result["result"]
    assert "AWAIT_LOCK" in result["result"]

    bitcoind.generate_block(6)
    sync_blockheight(bitcoind, [l1, l2, l3])

    cl1 = l2.rpc.listpeerchannels(l1.info["id"])["channels"][0][
        "short_channel_id"
    ]
    cl2 = l2.rpc.listpeerchannels(l3.info["id"])["channels"][0][
        "short_channel_id"
    ]
    l2.wait_channel_active(cl1)
    l2.wait_channel_active(cl2)

    result = l2.rpc.call("summars")
    assert "OK" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "OK"})
    assert "OK" not in result["result"]
    assert "1 channel filtered" in result["result"]

    l1.rpc.close(cl1)

    result = l2.rpc.call("summars")
    assert "CLOSINGD_DONE" in result["result"]
    assert "OK" in result["result"]


def test_flowtables(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2, l3 = node_factory.get_nodes(3, opts={"plugin": get_plugin})
    l1.fundwallet(10_000_000)
    l2.fundwallet(10_000_000)
    l1.rpc.connect(l2.info["id"], "localhost", l2.port)
    l2.rpc.connect(l3.info["id"], "localhost", l3.port)
    l1.rpc.connect(l3.info["id"], "localhost", l3.port)
    l1.rpc.fundchannel(l2.info["id"], 1_000_000, mindepth=1)
    l2.rpc.fundchannel(l3.info["id"], 1_000_000, mindepth=1)

    bitcoind.generate_block(6)
    sync_blockheight(bitcoind, [l1, l2, l3])

    cl1 = l2.rpc.listpeerchannels(l1.info["id"])["channels"][0][
        "short_channel_id"
    ]
    cl2 = l2.rpc.listpeerchannels(l3.info["id"])["channels"][0][
        "short_channel_id"
    ]
    l2.wait_channel_active(cl1)
    l2.wait_channel_active(cl2)

    result = l2.rpc.call("summars")
    assert "OK" in result["result"]

    routel1l2l3 = [
        {
            "id": l1.info["id"],
            "short_channel_id": cl1,
            "fee_base_msat": 1000,
            "fee_proportional_millionths": 10,
            "cltv_expiry_delta": 6,
        },
        {
            "id": l2.info["id"],
            "short_channel_id": cl2,
            "fee_base_msat": 1000,
            "fee_proportional_millionths": 10,
            "cltv_expiry_delta": 6,
        },
    ]
    inv = l3.dev_invoice(
        amount_msat=123_000,
        label="test_pay_routeboost2",
        description="test_pay_routeboost2",
        dev_routes=[routel1l2l3],
    )
    l1.dev_pay(inv["bolt11"], dev_use_shadow=False)

    result = l2.rpc.call("summars", {"summars-forwards": 1})
    assert "123" in result["result"]

    result = l1.rpc.call("summars", {"summars-pays": 1})
    assert "124" in result["result"]

    result = l3.rpc.call("summars", {"summars-invoices": 1})
    assert "123" in result["result"]
