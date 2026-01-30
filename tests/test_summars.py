#!/usr/bin/python

import os
import threading

import pytest
from pyln.client import RpcError
from pyln.testing.fixtures import *  # noqa: F403
from pyln.testing.utils import only_one, sync_blockheight, wait_for
from pathlib import Path
from util import get_plugin, my_xpay, new_preimage  # noqa: F401

columns = [
    "GRAPH_SATS",
    "OUT_SATS",
    "IN_SATS",
    "TOTAL_SATS",
    "SCID",
    "MIN_HTLC",
    "MAX_HTLC",
    "FLAG",
    "BASE",
    "IN_BASE",
    "PPM",
    "IN_PPM",
    "ALIAS",
    "PEER_ID",
    "UPTIME",
    "HTLCS",
    "STATE",
    "PERC_US",
    "PING",
]

forwards_columns = [
    "received_time",
    "resolved_time",
    "in_channel",
    "out_channel",
    "in_alias",
    "out_alias",
    "in_sats",
    "out_sats",
    "fee_msats",
    "eff_fee_ppm",
]

pay_columns = [
    "completed_at",
    "payment_hash",
    "sats_requested",
    "sats_sent",
    "fee_msats",
    "destination",
    "description",
    "preimage",
]

invoice_columns = [
    "paid_at",
    "label",
    "description",
    "sats_received",
    "payment_hash",
    "preimage",
]


def test_basic(node_factory, get_plugin):  # noqa: F811
    node = node_factory.get_node(options={"plugin": get_plugin, "log-level": "debug"})
    result = node.rpc.call("summars", {"summars-locale": "en-US"})
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

    expected_columns = [
        x
        for x in columns
        if x != "GRAPH_SATS"
        and x != "PERC_US"
        and x != "TOTAL_SATS"
        and x != "MIN_HTLC"
        and x != "IN_BASE"
        and x != "IN_PPM"
        and x != "PING"
    ]
    for column in expected_columns:
        assert column in result["result"]

    unexpected_columns = ["GRAPH_SATS"]
    for column in unexpected_columns:
        assert column not in result["result"]

    with pytest.raises(RpcError, match="not a valid string"):
        node.rpc.call("summars", {"summars-columns": 1})

    with pytest.raises(
        RpcError, match="`TEST` not found in valid summars-columns names"
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
    node = node_factory.get_node(options={"plugin": get_plugin, "log-level": "debug"})

    for col in columns:
        result = node.rpc.call("summars", {"summars-columns": col})
        assert " " + col + " " in result["result"]
        for col2 in columns:
            if col != col2:
                assert " " + col2 + " " not in result["result"]
        result = node.rpc.call("summars", {"summars-columns": col.lower()})
        assert " " + col in result["result"]
        for col2 in columns:
            if col != col2:
                assert " " + col2 + " " not in result["result"]
    result = node.rpc.call(
        "summars",
        {"summars-columns": "PPM,PEER_ID,IN_SATS,SCID"},
    )
    ppm = result["result"].find("PPM")
    peer_id = result["result"].find("PEER_ID")
    in_sats = result["result"].find("IN_SATS")
    scid = result["result"].find("SCID")
    assert ppm != -1
    assert peer_id != -1
    assert in_sats != -1
    assert scid != -1
    assert ppm < peer_id and peer_id < in_sats and in_sats < scid

    for col in pay_columns:
        result = node.rpc.call(
            "summars", {"summars-pays": 1, "summars-pays-columns": col}
        )
        assert col in result["result"]
        for col2 in pay_columns:
            if col != col2:
                assert col2 not in result["result"]
        result = node.rpc.call(
            "summars", {"summars-pays": 1, "summars-pays-columns": col.upper()}
        )
        assert col in result["result"]
        for col2 in pay_columns:
            if col != col2:
                assert col2 not in result["result"]
    result = node.rpc.call(
        "summars",
        {
            "summars-pays": 1,
            "summars-pays-columns": "description,destination,sats_sent,preimage",
        },
    )
    description = result["result"].find("description")
    destination = result["result"].find("destination")
    sats_sent = result["result"].find("sats_sent")
    preimage = result["result"].find("preimage")
    assert description != -1
    assert destination != -1
    assert sats_sent != -1
    assert preimage != -1
    assert (
        description < destination and destination < sats_sent and sats_sent < preimage
    )

    for col in invoice_columns:
        result = node.rpc.call(
            "summars", {"summars-invoices": 1, "summars-invoices-columns": col}
        )
        assert col in result["result"]
        for col2 in invoice_columns:
            if col != col2:
                assert col2 not in result["result"]
        result = node.rpc.call(
            "summars",
            {"summars-invoices": 1, "summars-invoices-columns": col.upper()},
        )
        assert col in result["result"]
        for col2 in invoice_columns:
            if col != col2:
                assert col2 not in result["result"]

    result = node.rpc.call(
        "summars",
        {
            "summars-invoices": 1,
            "summars-invoices-columns": "sats_received,description,label,paid_at",
        },
    )
    sats_received = result["result"].find("sats_received")
    description = result["result"].find("description")
    label = result["result"].find("label")
    paid_at = result["result"].find("paid_at")
    assert sats_received != -1
    assert description != -1
    assert label != -1
    assert paid_at != -1
    assert sats_received < description and description < label and label < paid_at

    for col in forwards_columns:
        result = node.rpc.call(
            "summars", {"summars-forwards": 1, "summars-forwards-columns": col}
        )
        assert col in result["result"]
        for col2 in forwards_columns:
            if col != col2:
                assert col2 not in result["result"]
        result = node.rpc.call(
            "summars",
            {"summars-forwards": 1, "summars-forwards-columns": col.upper()},
        )
        assert col in result["result"]
        for col2 in forwards_columns:
            if col != col2:
                assert col2 not in result["result"]

    result = node.rpc.call(
        "summars",
        {
            "summars-forwards": 1,
            "summars-forwards-columns": "in_sats,in_channel,resolved_time,out_channel,out_sats",
        },
    )
    in_sats = result["result"].find("in_sats")
    in_channel = result["result"].find("in_channel")
    resolved_time = result["result"].find("resolved_time")
    out_channel = result["result"].find("out_channel")
    out_sats = result["result"].find("out_sats")
    assert in_sats != -1
    assert in_channel != -1
    assert resolved_time != -1
    assert paid_at != -1
    assert out_sats != -1
    assert (
        in_sats < in_channel
        and in_channel < resolved_time
        and resolved_time < out_channel
        and out_channel < out_sats
    )

    for col in columns:
        if col == "GRAPH_SATS":
            with pytest.raises(RpcError, match="Can not sort by `GRAPH_SATS`"):
                node.rpc.call(
                    "summars",
                    {
                        "summars-columns": ",".join(columns),
                        "summars-sort-by": col,
                    },
                )
        else:
            result = node.rpc.call(
                "summars",
                {"summars-columns": ",".join(columns), "summars-sort-by": col},
            )
            assert col in result["result"]
        if col == "GRAPH_SATS":
            with pytest.raises(RpcError, match="Can not sort by `GRAPH_SATS`"):
                node.rpc.call(
                    "summars",
                    {
                        "summars-columns": ",".join(columns),
                        "summars-sort-by": col.lower(),
                    },
                )
        else:
            result = node.rpc.call(
                "summars",
                {
                    "summars-columns": ",".join(columns),
                    "summars-sort-by": col.lower(),
                },
            )
            assert col in result["result"]

    result = node.rpc.call("summars", {"summars-exclude-states": "OK"})
    assert "OK" not in result["result"]

    result = node.rpc.call("summars", {"summars-forwards": 1})
    assert "forwards (last 1h, limit: off)" in result["result"]

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
        "summars",
        {"summars-forwards": 1, "summars-forwards-filter-amount-msat": -1},
    )
    assert "forwards" in result["result"]

    result = node.rpc.call(
        "summars",
        {"summars-forwards": 1, "summars-forwards-filter-fee-msat": -1},
    )
    assert "forwards" in result["result"]

    result = node.rpc.call("summars", {"summars-pays": 1})
    assert "pays (last 1h, limit: off)" in result["result"]

    result = node.rpc.call("summars", {"summars-invoices": 1})
    assert "invoices (last 1h, limit: off)" in result["result"]

    result = node.rpc.call(
        "summars",
        {"summars-invoices": 1, "summars-invoices-filter-amount-msat": 1},
    )
    assert "invoices" in result["result"]

    result = node.rpc.call(
        "summars",
        {"summars-invoices": 1, "summars-invoices-filter-amount-msat": -1},
    )
    assert "invoices" in result["result"]

    result = node.rpc.call("summars", {"summars-locale": "de"})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-max-alias-length": 5})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-max-description-length": 5})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-max-label-length": 5})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-utf8": False})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-style": "modern"})
    assert "result" in result

    result = node.rpc.call("summars", {"summars-flow-style": "modern"})
    assert "result" in result

    result = node.rpc.call(
        "summars", {"summars-forwards": 24, "summars-forwards-limit": 100}
    )
    assert "forwards (last 24h, limit: 0/100)" in result["result"]
    result = node.rpc.call(
        "summars", {"summars-invoices": 24, "summars-invoices-limit": 100}
    )
    assert "invoices (last 24h, limit: 0/100)" in result["result"]
    result = node.rpc.call("summars", {"summars-pays": 24, "summars-pays-limit": 100})
    assert "pays (last 24h, limit: 0/100)" in result["result"]


def test_option_errors(node_factory, get_plugin):  # noqa: F811
    node = node_factory.get_node(options={"plugin": get_plugin, "log-level": "debug"})

    with pytest.raises(RpcError, match="not found in valid summars-columns names"):
        node.rpc.call("summars", {"summars-columns": "test"})
    with pytest.raises(RpcError, match="Duplicate entry"):
        node.rpc.call("summars", {"summars-columns": "IN_SATS,IN_SATS"})
    with pytest.raises(RpcError, match="not found in valid summars-columns names"):
        node.rpc.call("summars", {"summars-columns": "PRIVATE"})
    with pytest.raises(RpcError, match="not found in valid summars-columns names"):
        node.rpc.call("summars", {"summars-columns": "OFFLINE"})

    with pytest.raises(
        RpcError, match="not found in valid summars-forwards-columns names"
    ):
        node.rpc.call("summars", {"summars-forwards-columns": "test"})
    with pytest.raises(RpcError, match="Duplicate entry"):
        node.rpc.call("summars", {"summars-forwards-columns": "in_channel,in_channel"})

    with pytest.raises(RpcError, match="not found in valid summars-pays-columns names"):
        node.rpc.call("summars", {"summars-pays-columns": "test"})
    with pytest.raises(RpcError, match="Duplicate entry"):
        node.rpc.call("summars", {"summars-pays-columns": "description,description"})

    with pytest.raises(
        RpcError, match="not found in valid summars-invoices-columns names"
    ):
        node.rpc.call("summars", {"summars-invoices-columns": "test"})
    with pytest.raises(RpcError, match="Duplicate entry"):
        node.rpc.call(
            "summars", {"summars-invoices-columns": "description, description"}
        )

    with pytest.raises(RpcError, match="does not make sense"):
        node.rpc.call("summars", {"summars-refresh-alias": 1})

    with pytest.raises(RpcError, match="does not make sense"):
        node.rpc.call("summars", {"summars-availability-interval": 1})

    with pytest.raises(RpcError, match="does not make sense"):
        node.rpc.call("summars", {"summars-availability-window": 1})

    with pytest.raises(RpcError, match="not a valid string"):
        node.rpc.call("summars", {"summars-sort-by": 1})
    with pytest.raises(RpcError, match="is invalid. Can only sort by valid columns."):
        node.rpc.call("summars", {"summars-sort-by": "TEST"})

    node.rpc.call("summars", {"summars-sort-by": "IN_PPM"})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-forwards": "TEST"})
    with pytest.raises(RpcError, match="needs to be a positive number"):
        node.rpc.call("summars", {"summars-forwards": -1})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-forwards-filter-amount-msat": "TEST"})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-forwards-filter-fee-msat": "TEST"})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-pays": "TEST"})
    with pytest.raises(RpcError, match="needs to be a positive number"):
        node.rpc.call("summars", {"summars-pays": -1})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-invoices": "TEST"})
    with pytest.raises(RpcError, match="needs to be a positive number"):
        node.rpc.call("summars", {"summars-invoices": -1})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-invoices-filter-amount-msat": "TEST"})

    with pytest.raises(RpcError, match="not a valid string"):
        node.rpc.call("summars", {"summars-locale": -1})
    with pytest.raises(RpcError, match="not a valid locale"):
        node.rpc.call("summars", {"summars-locale": "xxxx"})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-max-alias-length": "TEST"})
    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-max-alias-length": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-max-alias-length": 4})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-max-description-length": "TEST"})
    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-max-description-length": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-max-description-length": 4})

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.call("summars", {"summars-max-label-length": "TEST"})
    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-max-label-length": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-max-label-length": 4})

    with pytest.raises(RpcError, match="not a valid boolean"):
        node.rpc.call("summars", {"summars-utf8": "TEST"})
    with pytest.raises(RpcError, match="not a valid boolean"):
        node.rpc.call("summars", {"summars-utf8": 1})

    with pytest.raises(RpcError, match="not a valid string"):
        node.rpc.call("summars", {"summars-style": 1})
    with pytest.raises(RpcError, match="could not parse Style"):
        node.rpc.call("summars", {"summars-style": "TEST"})

    with pytest.raises(RpcError, match="not a valid string"):
        node.rpc.call("summars", {"summars-flow-style": 1})
    with pytest.raises(RpcError, match="could not parse Style"):
        node.rpc.call("summars", {"summars-flow-style": "TEST"})

    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-forwards-limit": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-pays-limit": -1})
    with pytest.raises(RpcError, match="must be greater than or equal to |"):
        node.rpc.call("summars", {"summars-invoices-limit": -1})

    with pytest.raises(
        RpcError,
        match="You must set `summars-forwards` for `summars-forwards-limit` to have an effect!",
    ):
        node.rpc.call("summars", {"summars-forwards-limit": 1})
    with pytest.raises(
        RpcError,
        match="You must set `summars-pays` for `summars-pays-limit` to have an effect!",
    ):
        node.rpc.call("summars", {"summars-pays-limit": 1})
    with pytest.raises(
        RpcError,
        match="You must set `summars-invoices` for `summars-invoices-limit` to have an effect!",
    ):
        node.rpc.call("summars", {"summars-invoices-limit": 1})


def test_setconfig_options(node_factory, get_plugin):  # noqa: F811
    node = node_factory.get_node(
        allow_broken_log=True,
        options={
            "plugin": get_plugin,
            "summars-forwards": 1,
            "summars-invoices": 1,
            "summars-pays": 1,
            "summars-utf8": True,
            "log-level": "debug",
        },
    )
    result = node.rpc.call("summars")
    assert "forwards" in result["result"]
    assert "pays" in result["result"]
    assert "invoices" in result["result"]

    with pytest.raises(RpcError, match="needs to be a positive number"):
        node.rpc.setconfig("summars-forwards", -1)
    node.rpc.setconfig("summars-forwards", 0)
    node.rpc.setconfig("summars-invoices", 0)
    node.rpc.setconfig("summars-pays", 0)
    result = node.rpc.call("summars")
    assert "forwards" not in result["result"]
    assert "pays" not in result["result"]
    assert "invoices" not in result["result"]

    node.rpc.setconfig("summars-columns", "IN_SATS, OUT_SATS")
    result = node.rpc.call("summars")
    assert "ALIAS" not in result["result"]
    assert "PEER_ID" not in result["result"]
    assert "STATE" not in result["result"]

    with pytest.raises(RpcError, match="is invalid. Can only sort by valid columns."):
        node.rpc.setconfig("summars-sort-by", 1)
    with pytest.raises(RpcError, match="is invalid. Can only sort by valid columns."):
        node.rpc.setconfig("summars-sort-by", "TEST")

    with pytest.raises(RpcError, match="not a valid integer"):
        node.rpc.setconfig("summars-forwards", "TEST")

    with pytest.raises(RpcError) as err:
        node.rpc.setconfig("summars-utf8", "test")
    assert err.value.error["message"] == "summars-utf8 is not a valid boolean!"
    assert err.value.error["code"] == -32602
    assert (
        node.rpc.listconfigs("summars-utf8")["configs"]["summars-utf8"]["value_bool"]
        != "test"
    )
    node.rpc.setconfig("summars-utf8", False)
    assert (
        node.rpc.listconfigs("summars-utf8")["configs"]["summars-utf8"]["value_bool"]
        is False
    )


def test_chanstates(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2, l3 = node_factory.get_nodes(
        3,
        opts=[
            {"plugin": get_plugin, "log-level": "debug"},
            {"log-level": "debug"},
            {"log-level": "debug"},
        ],
    )
    l1.fundwallet(10_000_000)
    l2.fundwallet(10_000_000)
    l1.rpc.fundchannel(
        l2.info["id"] + "@127.0.0.1:" + str(l2.port), 1_000_000, mindepth=1
    )
    bitcoind.generate_block(1)
    sync_blockheight(bitcoind, [l1, l2, l3])
    l1.rpc.fundchannel(
        l3.info["id"] + "@127.0.0.1:" + str(l3.port),
        1_000_000,
        mindepth=1,
        announce=False,
    )
    result = l1.rpc.call("summars")
    assert l1.info["id"] in result["result"]
    assert "AWAIT_LOCK" in result["result"]

    bitcoind.generate_block(6)
    sync_blockheight(bitcoind, [l1, l2, l3])

    chans = l1.rpc.listpeerchannels(l2.info["id"])["channels"]

    for chan in chans:
        if chan["private"]:
            l1.wait_local_channel_active(chan["short_channel_id"])
        else:
            l1.wait_channel_active(chan["short_channel_id"])

    result = l1.rpc.call("summars")
    assert "OK" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "OK"})
    assert "OK" not in result["result"]
    assert "2 channels filtered" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "PRIVATE"})
    assert "[P" not in result["result"]
    assert "1 channel filtered" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "PUBLIC"})
    assert "[_" not in result["result"]
    assert "1 channel filtered" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "ONLINE"})
    assert "_]" not in result["result"]
    assert "2 channels filtered" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "ok"})
    assert "OK" not in result["result"]
    assert "2 channels filtered" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "private"})
    assert "[P" not in result["result"]
    assert "1 channel filtered" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "public"})
    assert "[_" not in result["result"]
    assert "1 channel filtered" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "online"})
    assert "_]" not in result["result"]
    assert "2 channels filtered" in result["result"]

    l3.stop()

    wait_for(
        lambda: not only_one(l1.rpc.listpeerchannels(l3.info["id"])["channels"])[
            "peer_connected"
        ]
    )
    result = l1.rpc.call("summars", {"summars-exclude-states": "OFFLINE"})
    assert "O]" not in result["result"]
    assert "1 channel filtered" in result["result"]

    result = l1.rpc.call("summars", {"summars-exclude-states": "offline"})
    assert "O]" not in result["result"]
    assert "1 channel filtered" in result["result"]

    l1.rpc.close(chans[0]["short_channel_id"])

    wait_for(
        lambda: only_one(l1.rpc.listpeerchannels(l2.info["id"])["channels"])["state"]
        == "CLOSINGD_COMPLETE"
    )
    result = l1.rpc.call("summars")
    assert "CLOSINGD_DONE" in result["result"]
    assert "OK" in result["result"]


def test_flowtables(node_factory, bitcoind, get_plugin):  # noqa: F811
    l1, l2, l3 = node_factory.get_nodes(
        3, opts={"plugin": get_plugin, "log-level": "debug"}
    )
    l1.fundwallet(10_000_000)
    l2.fundwallet(10_000_000)
    l1.rpc.connect(l2.info["id"], "127.0.0.1", l2.port)
    l2.rpc.connect(l3.info["id"], "127.0.0.1", l3.port)
    l1.rpc.connect(l3.info["id"], "127.0.0.1", l3.port)
    l1.rpc.fundchannel(l2.info["id"], 2_000_000, push_msat=1_000_000_000, mindepth=1)
    l2.rpc.fundchannel(l3.info["id"], 2_000_000, push_msat=1_000_000_000, mindepth=1)

    bitcoind.generate_block(6)
    sync_blockheight(bitcoind, [l1, l2, l3])

    cl1 = l2.rpc.listpeerchannels(l1.info["id"])["channels"][0]["short_channel_id"]
    cl2 = l2.rpc.listpeerchannels(l3.info["id"])["channels"][0]["short_channel_id"]
    l2.wait_channel_active(cl1)
    l2.wait_channel_active(cl2)

    result = l2.rpc.call("summars")
    assert "OK" in result["result"]

    routel1l2l3 = [
        {
            "id": l1.info["id"],
            "short_channel_id": cl1,
            "fee_base_msat": 1_000,
            "fee_proportional_millionths": 10,
            "cltv_expiry_delta": 6,
        },
        {
            "id": l2.info["id"],
            "short_channel_id": cl2,
            "fee_base_msat": 1_000,
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
    pay1 = l1.dev_pay(inv["bolt11"], dev_use_shadow=False)

    result = l2.rpc.call("summars", {"summars-forwards": 1})
    assert "123" in result["result"]

    result = l1.rpc.call("summars", {"summars-pays": 1})
    assert "124" in result["result"]

    result = l3.rpc.call("summars", {"summars-invoices": 1})
    assert "123" in result["result"]

    routel3l2l1 = [
        {
            "id": l3.info["id"],
            "short_channel_id": cl2,
            "fee_base_msat": 1_000,
            "fee_proportional_millionths": 10,
            "cltv_expiry_delta": 6,
        },
        {
            "id": l2.info["id"],
            "short_channel_id": cl1,
            "fee_base_msat": 1_000,
            "fee_proportional_millionths": 10,
            "cltv_expiry_delta": 6,
        },
    ]

    inv2 = l1.dev_invoice(
        amount_msat=223_000,
        label="test_pay_routeboost3",
        description="test_pay_routeboost3",
        dev_routes=[routel3l2l1],
    )
    l3.dev_pay(inv2["bolt11"], dev_use_shadow=False)

    inv3 = l1.dev_invoice(
        amount_msat=13_000,
        label="test_pay_routeboost4",
        description="test_pay_routeboost4",
        dev_routes=[routel3l2l1],
    )
    l3.dev_pay(inv3["bolt11"], dev_use_shadow=False)

    result = l1.rpc.call(
        "summars",
        {"summars-forwards": 1, "summars-pays": 1, "summars-invoices": 1},
    )
    assert "forwards" in result["result"]
    assert "pays" in result["result"]
    assert "invoices" in result["result"]
    assert "123 sats_requested 124 sats_sent 1 fee_sats" in result["result"]
    assert "236 sats_received" in result["result"]

    result = l1.rpc.call(
        "summars",
        {
            "summars-forwards": 1,
            "summars-pays": 1,
            "summars-invoices": 1,
            "summars-json": True,
        },
    )
    assert "info" in result
    assert "channels" in result
    assert "forwards" in result
    assert "pays" in result
    assert "invoices" in result
    assert pay1["payment_hash"] == result["pays"][0]["payment_hash"]
    assert 223_000 == result["invoices"][0]["msats_received"]
    assert result["totals"]["invoices"]["amount_received_msat"] == 236000
    assert result["totals"]["pays"]["amount_msat"] == 123000
    assert result["totals"]["pays"]["amount_sent_msat"] == 124001
    assert result["totals"]["pays"]["fees_msat"] == 1001

    result = l2.rpc.call(
        "summars",
        {
            "summars-forwards": 1,
            "summars-pays": 1,
            "summars-invoices": 1,
            "summars-json": True,
        },
    )
    assert len(result["forwards"]) == 3
    assert result["totals"]["forwards"]["amount_in_msat"] == 362003
    assert result["totals"]["forwards"]["amount_out_msat"] == 359000
    assert result["totals"]["forwards"]["fees_msat"] == 3003

    result = l2.rpc.call(
        "summars",
        {"summars-forwards": 1, "summars-pays": 1, "summars-invoices": 1},
    )
    assert "forwards" in result["result"]
    assert "pays" in result["result"]
    assert "invoices" in result["result"]
    assert (
        "Total of 3 forwards in the last 1h: 362 in_sats 359 out_sats 3 fee_sats"
        in result["result"]
    )

    result = l3.rpc.call(
        "summars",
        {
            "summars-forwards": 1,
            "summars-pays": 1,
            "summars-invoices": 1,
            "summars-json": True,
        },
    )
    assert result["totals"]["invoices"]["amount_received_msat"] == 123000
    assert result["totals"]["pays"]["amount_msat"] == 236000
    assert result["totals"]["pays"]["amount_sent_msat"] == 238002
    assert result["totals"]["pays"]["fees_msat"] == 2002


def test_indexing(node_factory, bitcoind, get_plugin):  # noqa: F811
    grpc_port = node_factory.get_unused_port()
    l1, l2, l3 = node_factory.get_nodes(
        3,
        opts=[
            {"log-level": "debug", "plugin": get_plugin},
            {"log-level": "debug", "plugin": get_plugin},
            {
                "log-level": "debug",
                "plugin": [
                    get_plugin,
                    os.path.join(Path(__file__).parent.resolve(), "hold"),
                ],
                "hold-grpc-port": grpc_port,
            },
        ],
    )
    l1.fundwallet(10_000_000)
    l2.fundwallet(10_000_000)
    l1.rpc.fundchannel(
        l2.info["id"] + "@127.0.0.1:" + str(l2.port),
        1_000_000,
        push_msat=500_000_000,
        mindepth=1,
    )
    l2.rpc.fundchannel(
        l3.info["id"] + "@127.0.0.1:" + str(l3.port),
        1_000_000,
        push_msat=500_000_000,
        mindepth=1,
    )
    bitcoind.generate_block(6)
    sync_blockheight(bitcoind, [l1, l2, l3])

    cl1 = l2.rpc.listpeerchannels(l1.info["id"])["channels"][0]["short_channel_id"]
    cl2 = l3.rpc.listpeerchannels(l2.info["id"])["channels"][0]["short_channel_id"]
    l1.wait_channel_active(cl1)
    l2.wait_channel_active(cl1)
    l2.wait_channel_active(cl2)
    l3.wait_channel_active(cl2)

    preimage, payment_hash = new_preimage()
    hold_inv = l3.rpc.call(
        "holdinvoice",
        {
            "amount": 1_000,
            "payment_hash": payment_hash,
        },
    )

    threading.Thread(target=my_xpay, args=(l1, hold_inv["bolt11"])).start()

    wait_for(
        lambda: l3.rpc.call("listholdinvoices", {"payment_hash": payment_hash})[
            "holdinvoices"
        ][0]["state"]
        == "accepted"
    )
    result = l1.rpc.call("summars", {"summars-pays": 1})
    assert payment_hash not in result["result"]
    result = l2.rpc.call("summars", {"summars-forwards": 1, "summars-json": True})
    assert len(result["forwards"]) == 0
    result = l3.rpc.call("summars", {"summars-invoices": 1})
    assert payment_hash not in result["result"]

    new_inv = l3.rpc.invoice(1_000, "new_inv", "new_inv")

    my_xpay(l1, new_inv["bolt11"])

    result = l1.rpc.call("summars", {"summars-pays": 1})
    assert new_inv["payment_hash"] in result["result"]
    assert payment_hash not in result["result"]
    result = l2.rpc.call("summars", {"summars-forwards": 1, "summars-json": True})
    assert len(result["forwards"]) == 1
    result = l3.rpc.call("summars", {"summars-invoices": 1})
    assert new_inv["payment_hash"] in result["result"]
    assert payment_hash not in result["result"]

    l3.rpc.call("settleholdinvoice", {"preimage": preimage})

    wait_for(
        lambda: l1.rpc.listpays(payment_hash=payment_hash)["pays"][0]["status"]
        == "complete"
    )

    wait_for(
        lambda: l3.rpc.listholdinvoices(payment_hash=payment_hash)["holdinvoices"][0][
            "state"
        ]
        == "paid"
    )

    result = l1.rpc.call("summars", {"summars-pays": 1})
    assert new_inv["payment_hash"] in result["result"]
    assert payment_hash in result["result"]
    result = l2.rpc.call("summars", {"summars-forwards": 1, "summars-json": True})
    assert len(result["forwards"]) == 2
    result = l3.rpc.call("summars", {"summars-invoices": 1})
    assert new_inv["payment_hash"] in result["result"]
    assert payment_hash in result["result"]

    new_inv2 = l3.rpc.invoice(1_000, "new_inv2", "new_inv2")

    my_xpay(l1, new_inv2["bolt11"])

    result = l1.rpc.call("summars", {"summars-pays": 1})
    assert new_inv["payment_hash"] in result["result"]
    assert new_inv2["payment_hash"] in result["result"]
    assert payment_hash in result["result"]
    result = l2.rpc.call("summars", {"summars-forwards": 1, "summars-json": True})
    assert len(result["forwards"]) == 3
    result = l3.rpc.call("summars", {"summars-invoices": 1})
    assert new_inv["payment_hash"] in result["result"]
    assert new_inv2["payment_hash"] in result["result"]
    assert payment_hash in result["result"]
