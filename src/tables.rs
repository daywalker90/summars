use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::primitives::ShortChannelId;
use cln_rpc::{
    model::requests::*,
    model::responses::*,
    primitives::{Amount, PublicKey},
};

use log::debug;
use std::collections::BTreeMap;
use std::str::FromStr;
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::settings::location::ByColumnName;
use tabled::settings::object::{Object, Rows};
use tabled::settings::{Alignment, Disable, Format, Modify, Width};

use serde_json::json;
use std::path::PathBuf;

use tabled::Table;
use tokio::time::Instant;

use crate::config::validateargs;
use crate::rpc::{
    get_info, list_forwards, list_funds, list_invoices, list_nodes, list_pays, list_peer_channels,
    list_peers,
};
use crate::structs::{
    ChannelVisibility, Config, Forwards, Invoices, PagingIndex, Pays, PluginState,
    ShortChannelState, Summary, NODE_GOSSIP_MISS, NO_ALIAS_SET,
};
use crate::util::{
    draw_chans_graph, is_active_state, make_channel_flags, make_rpc_path,
    timestamp_to_localized_datetime_string, u64_to_btc_string, u64_to_sat_string,
};

pub async fn summary(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let now = Instant::now();

    let rpc_path = make_rpc_path(&p);

    let mut config = p.state().config.lock().clone();
    validateargs(v, &mut config)?;

    let getinfo = get_info(&rpc_path).await?;
    debug!(
        "Getinfo. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let peers = list_peers(&rpc_path).await?.peers;
    debug!(
        "Listpeers. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    let peer_channels = list_peer_channels(&rpc_path)
        .await?
        .channels
        .ok_or(anyhow!("list_peer_channels returned with None!"))?;
    debug!(
        "Listpeerchannels. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let funds = list_funds(&rpc_path).await?;
    debug!(
        "Listfunds. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let mut utxo_amt: u64 = 0;
    for utxo in &funds.outputs {
        if let ListfundsOutputsStatus::CONFIRMED = utxo.status {
            utxo_amt += Amount::msat(&utxo.amount_msat)
        }
    }

    let max_chan_sides: Vec<u64> = peer_channels
        .iter()
        .flat_map(|channel| {
            vec![
                Amount::msat(&channel.to_us_msat.unwrap_or(Amount::from_msat(0))),
                Amount::msat(&channel.total_msat.unwrap_or(Amount::from_msat(0))).saturating_sub(
                    Amount::msat(&channel.to_us_msat.unwrap_or(Amount::from_msat(0))),
                ),
            ]
        })
        .collect();
    let graph_max_chan_side_msat = max_chan_sides
        .iter()
        .copied()
        .max()
        .unwrap_or(u64::default());

    let mut channel_count = 0;
    let mut num_connected = 0;
    let mut avail_in = 0;
    let mut avail_out = 0;

    let mut filter_count = 0;

    let mut table = Vec::new();

    let num_gossipers = peers
        .iter()
        .filter(|s| s.num_channels.unwrap() == 0)
        .count();

    for chan in &peer_channels {
        if config
            .exclude_channel_states
            .value
            .contains(&ShortChannelState(chan.state.unwrap()))
            || if let Some(excl_vis) = &config.exclude_pub_priv_states {
                match excl_vis {
                    ChannelVisibility::Private => chan.private.unwrap(),
                    ChannelVisibility::Public => !chan.private.unwrap(),
                }
            } else {
                false
            }
        {
            filter_count += 1;
            continue;
        }
        let alias = get_alias(&rpc_path, p.clone(), chan.peer_id.unwrap()).await?;

        let to_us_msat = Amount::msat(&chan.to_us_msat.ok_or(anyhow!(
            "Channel with {} has no msats to us!",
            chan.peer_id.unwrap()
        ))?);
        let total_msat = Amount::msat(&chan.total_msat.ok_or(anyhow!(
            "Channel with {} has no total amount!",
            chan.peer_id.unwrap()
        ))?);
        let our_reserve = Amount::msat(&chan.our_reserve_msat.ok_or(anyhow!(
            "Channel with {} has no our_reserve!",
            chan.peer_id.unwrap()
        ))?);
        let their_reserve = Amount::msat(&chan.their_reserve_msat.ok_or(anyhow!(
            "Channel with {} has no their_reserve!",
            chan.peer_id.unwrap()
        ))?);

        if matches!(
            chan.state.unwrap(),
            ListpeerchannelsChannelsState::CHANNELD_NORMAL
                | ListpeerchannelsChannelsState::CHANNELD_AWAITING_SPLICE
        ) {
            if our_reserve < to_us_msat {
                avail_out += to_us_msat - our_reserve
            }
            if their_reserve < total_msat - to_us_msat {
                avail_in += total_msat - to_us_msat - their_reserve
            }
        }

        let avail = match p.state().avail.lock().get(&chan.peer_id.unwrap()) {
            Some(a) => a.avail,
            None => -1.0,
        };

        let summary = chan_to_summary(
            &config,
            chan,
            alias,
            avail,
            to_us_msat,
            total_msat,
            graph_max_chan_side_msat,
        )?;
        table.push(summary);

        if is_active_state(chan) {
            if chan.peer_connected.unwrap() {
                num_connected += 1
            }
            channel_count += 1;
        }
    }
    debug!(
        "First summary-loop. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    sort_summary(&config, &mut table);
    debug!(
        "Sort summary. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let mut sumtable = Table::new(table);
    format_summary(&config, &mut sumtable);
    debug!(
        "Format summary. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let forwards;
    if config.forwards.value > 0 {
        forwards = Some(recent_forwards(&rpc_path, &peer_channels, p.clone(), &config, now).await?);
        debug!(
            "End of forwards table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        forwards = None;
    }

    let pays;
    if config.pays.value > 0 {
        pays = Some(recent_pays(&rpc_path, p.clone(), &config, now, getinfo.id).await?);
        debug!(
            "End of pays table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        pays = None;
    }

    let invoices;
    if config.invoices.value > 0 {
        invoices = Some(recent_invoices(p.clone(), &rpc_path, &config, now).await?);
        debug!(
            "End of invoices table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        invoices = None;
    }

    let addr_str = get_addrstr(&getinfo);

    let mut result = sumtable.to_string();
    if filter_count > 0 {
        result += &format!(
            "\n {} channel{} filtered.",
            filter_count,
            if filter_count == 1 { "" } else { "s" }
        )
    }
    if let Some(fws) = forwards {
        result += &("\n\n".to_owned() + &fws);
    }
    if let Some(p) = pays {
        result += &("\n\n".to_owned() + &p);
    }
    if let Some(i) = invoices {
        result += &("\n\n".to_owned() + &i);
    }

    Ok(json!({"format-hint":"simple","result":format!(
        "address={}
num_utxos={}
utxo_amount={} BTC
num_channels={}
num_connected={}
num_gossipers={}
avail_out={} BTC
avail_in={} BTC
fees_collected={} BTC
channels_flags=P:private O:offline
{}",
        addr_str,
        funds.outputs.len(),
        u64_to_btc_string(&config, utxo_amt)?,
        channel_count,
        num_connected,
        num_gossipers,
        u64_to_btc_string(&config, avail_out)?,
        u64_to_btc_string(&config, avail_in)?,
        u64_to_btc_string(&config, Amount::msat(&getinfo.fees_collected_msat))?,
        result,
    )}))
}

async fn recent_forwards(
    rpc_path: &PathBuf,
    peer_channels: &[ListpeerchannelsChannels],
    plugin: Plugin<PluginState>,
    config: &Config,
    now: Instant,
) -> Result<String, Error> {
    let now_utc = Utc::now().timestamp() as u64;
    {
        if plugin.state().fw_index.lock().timestamp > now_utc - config.forwards.value * 60 * 60 {
            *plugin.state().fw_index.lock() = PagingIndex::new();
            debug!("fw_index: forwards-age increased, resetting index");
        }
    }
    let fw_index = plugin.state().fw_index.lock().clone();
    debug!(
        "fw_index: start:{} timestamp:{}",
        fw_index.start, fw_index.timestamp
    );
    let forwards = list_forwards(
        rpc_path,
        Some(ListforwardsStatus::SETTLED),
        None,
        None,
        Some(ListforwardsIndex::CREATED),
        Some(fw_index.start),
        None,
    )
    .await?
    .forwards;
    debug!(
        "List {} forwards. Total: {}ms",
        forwards.len(),
        now.elapsed().as_millis().to_string()
    );

    let chanmap: BTreeMap<ShortChannelId, ListpeerchannelsChannels> = peer_channels
        .iter()
        .filter_map(|s| s.short_channel_id.map(|id| (id, s.clone())))
        .collect();

    let alias_map = plugin.state().alias_map.lock();

    let mut table = Vec::new();
    let mut filter_amt_sum_msat = 0;
    let mut filter_fee_sum_msat = 0;
    let mut filter_count = 0;
    let mut new_fw_index = PagingIndex {
        start: u64::MAX,
        timestamp: now_utc - config.forwards.value * 60 * 60,
    };
    for forward in forwards {
        if forward.received_time as u64 > now_utc - config.forwards.value * 60 * 60 {
            let inchan = if config.forwards_alias.value {
                match chanmap.get(&forward.in_channel) {
                    Some(chan) => match alias_map.get::<PublicKey>(&chan.peer_id.unwrap()) {
                        Some(alias) => {
                            if alias.eq(NO_ALIAS_SET) {
                                forward.in_channel.to_string()
                            } else {
                                alias.clone()
                            }
                        }
                        None => forward.in_channel.to_string(),
                    },
                    None => forward.in_channel.to_string(),
                }
            } else {
                forward.in_channel.to_string()
            };
            let fw_outchan = forward.out_channel.unwrap();
            let outchan = if config.forwards_alias.value {
                match chanmap.get(&fw_outchan) {
                    Some(chan) => match alias_map.get::<PublicKey>(&chan.peer_id.unwrap()) {
                        Some(alias) => {
                            if alias.eq(NO_ALIAS_SET) {
                                fw_outchan.to_string()
                            } else {
                                alias.clone()
                            }
                        }
                        None => fw_outchan.to_string(),
                    },
                    None => fw_outchan.to_string(),
                }
            } else {
                fw_outchan.to_string()
            };

            let mut should_filter = false;
            if forward.in_msat.msat() as i64 <= config.forwards_filter_amt_msat.value {
                should_filter = true;
            }
            if forward.fee_msat.unwrap().msat() as i64 <= config.forwards_filter_fee_msat.value {
                should_filter = true;
            }

            if should_filter {
                filter_amt_sum_msat += forward.in_msat.msat();
                filter_fee_sum_msat += forward.fee_msat.unwrap().msat();
                filter_count += 1;
            } else {
                table.push(Forwards {
                    received: (forward.received_time * 1000.0) as u64,
                    received_str: timestamp_to_localized_datetime_string(
                        config,
                        forward.received_time as u64,
                    )?,
                    in_channel: if config.utf8.value {
                        inchan
                    } else {
                        inchan.replace(|c: char| !c.is_ascii(), "?")
                    },
                    out_channel: if config.utf8.value {
                        outchan
                    } else {
                        outchan.replace(|c: char| !c.is_ascii(), "?")
                    },
                    in_sats: u64_to_sat_string(config, Amount::msat(&forward.in_msat) / 1000)?,
                    out_sats: u64_to_sat_string(
                        config,
                        Amount::msat(&forward.out_msat.unwrap()) / 1000,
                    )?,
                    fee_msats: u64_to_sat_string(config, Amount::msat(&forward.fee_msat.unwrap()))?,
                })
            }

            if let Some(c_index) = forward.created_index {
                if c_index < new_fw_index.start {
                    new_fw_index.start = c_index;
                }
            }
        }
    }
    if new_fw_index.start < u64::MAX {
        *plugin.state().fw_index.lock() = new_fw_index;
    }
    debug!(
        "Build forwards table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    table.sort_by_key(|x| x.received);
    let mut fwtable = Table::new(table);
    config.flow_style.value.apply(&mut fwtable);
    fwtable.with(
        Modify::new(ByColumnName::new("in_channel"))
            .with(Width::truncate(config.max_alias_length.value as usize).suffix("[..]")),
    );
    fwtable.with(
        Modify::new(ByColumnName::new("out_channel"))
            .with(Width::truncate(config.max_alias_length.value as usize).suffix("[..]")),
    );
    fwtable.with(Modify::new(ByColumnName::new("in_sats")).with(Alignment::right()));
    fwtable.with(Modify::new(ByColumnName::new("out_sats")).with(Alignment::right()));
    fwtable.with(Modify::new(ByColumnName::new("fee_msats")).with(Alignment::right()));

    if filter_count > 0 {
        let filter_sum_result = format!(
            "\nFiltered: {} forwards with {} sats routed and {} msat fees.",
            filter_count,
            filter_amt_sum_msat / 1000,
            filter_fee_sum_msat
        );
        Ok(fwtable.to_string() + &filter_sum_result)
    } else {
        Ok(fwtable.to_string())
    }
}

async fn recent_pays(
    rpc_path: &PathBuf,
    plugin: Plugin<PluginState>,
    config: &Config,
    now: Instant,
    mypubkey: PublicKey,
) -> Result<String, Error> {
    let pays = list_pays(rpc_path, Some(ListpaysStatus::COMPLETE))
        .await?
        .pays;
    debug!(
        "List pays. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    let mut table = Vec::new();
    for pay in pays {
        if pay.completed_at.unwrap() > Utc::now().timestamp() as u64 - config.pays.value * 60 * 60
            && pay.destination.unwrap() != mypubkey
        {
            let destination = get_alias(rpc_path, plugin.clone(), pay.destination.unwrap()).await?;
            table.push(Pays {
                completed_at: pay.completed_at.unwrap(),
                completed_at_str: timestamp_to_localized_datetime_string(
                    config,
                    pay.completed_at.unwrap(),
                )?,
                payment_hash: pay.payment_hash.to_string(),
                sats_sent: u64_to_sat_string(
                    config,
                    Amount::msat(&pay.amount_sent_msat.unwrap()) / 1_000,
                )?,
                destination: if destination == NODE_GOSSIP_MISS {
                    pay.destination.unwrap().to_string()
                } else if config.utf8.value {
                    destination
                } else {
                    destination.replace(|c: char| !c.is_ascii(), "?")
                },
            })
        }
    }
    debug!(
        "Build pays table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    table.sort_by_key(|x| x.completed_at);
    let mut paystable = Table::new(table);
    config.flow_style.value.apply(&mut paystable);
    paystable.with(Modify::new(ByColumnName::new("sats_sent")).with(Alignment::right()));
    Ok(paystable.to_string())
}

async fn recent_invoices(
    plugin: Plugin<PluginState>,
    rpc_path: &PathBuf,
    config: &Config,
    now: Instant,
) -> Result<String, Error> {
    let now_utc = Utc::now().timestamp() as u64;
    {
        if plugin.state().inv_index.lock().timestamp > now_utc - config.invoices.value * 60 * 60 {
            *plugin.state().inv_index.lock() = PagingIndex::new();
            debug!("inv_index: invoices-age increased, resetting index");
        }
    }
    let inv_index = plugin.state().inv_index.lock().clone();
    debug!(
        "inv_index: start:{} timestamp:{}",
        inv_index.start, inv_index.timestamp
    );
    let invoices = list_invoices(
        rpc_path,
        None,
        None,
        Some(ListinvoicesIndex::CREATED),
        Some(inv_index.start),
        None,
    )
    .await?
    .invoices;
    debug!(
        "List {} invoices. Total: {}ms",
        invoices.len(),
        now.elapsed().as_millis().to_string()
    );
    let mut table = Vec::new();
    let mut filter_count = 0;
    let mut filter_amt_sum_msat = 0;
    let mut new_inv_index = PagingIndex {
        start: u64::MAX,
        timestamp: now_utc - config.invoices.value * 60 * 60,
    };
    for invoice in invoices {
        if let ListinvoicesInvoicesStatus::PAID = invoice.status {
            let inv_paid_at = if let Some(p_at) = invoice.paid_at {
                p_at
            } else {
                continue;
            };
            if inv_paid_at > now_utc - config.invoices.value * 60 * 60 {
                if invoice.amount_received_msat.unwrap().msat() as i64
                    <= config.invoices_filter_amt_msat.value
                {
                    filter_count += 1;
                    filter_amt_sum_msat += invoice.amount_received_msat.unwrap().msat();
                } else {
                    table.push(Invoices {
                        paid_at: invoice.paid_at.unwrap(),
                        paid_at_str: timestamp_to_localized_datetime_string(
                            config,
                            invoice.paid_at.unwrap(),
                        )?,
                        label: invoice.label,
                        sats_received: u64_to_sat_string(
                            config,
                            Amount::msat(&invoice.amount_received_msat.unwrap()) / 1_000,
                        )?,
                    });
                }
                if let Some(c_index) = invoice.created_index {
                    if c_index < new_inv_index.start {
                        new_inv_index.start = c_index;
                    }
                }
            }
        }
    }
    if new_inv_index.start < u64::MAX {
        *plugin.state().inv_index.lock() = new_inv_index;
    }
    debug!(
        "Build invoices table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    table.sort_by_key(|x| x.paid_at);
    let mut invoicestable = Table::new(table);
    config.flow_style.value.apply(&mut invoicestable);
    invoicestable.with(Modify::new(ByColumnName::new("sats_received")).with(Alignment::right()));

    if filter_count > 0 {
        let filter_sum_result = format!(
            "\nFiltered: {} invoices with {} sats total.",
            filter_count,
            filter_amt_sum_msat / 1000
        );
        Ok(invoicestable.to_string() + &filter_sum_result)
    } else {
        Ok(invoicestable.to_string())
    }
}

async fn get_alias(
    rpc_path: &PathBuf,
    p: Plugin<PluginState>,
    peer_id: PublicKey,
) -> Result<String, Error> {
    let alias_map = p.state().alias_map.lock().clone();
    let alias;
    match alias_map.get::<PublicKey>(&peer_id) {
        Some(a) => alias = a.clone(),
        None => match list_nodes(rpc_path, &peer_id).await?.nodes.first() {
            Some(node) => {
                match &node.alias {
                    Some(newalias) => alias = newalias.clone(),
                    None => alias = NO_ALIAS_SET.to_string(),
                }
                p.state().alias_map.lock().insert(peer_id, alias.clone());
            }
            None => alias = NODE_GOSSIP_MISS.to_string(),
        },
    };
    Ok(alias)
}

fn chan_to_summary(
    config: &Config,
    chan: &ListpeerchannelsChannels,
    alias: String,
    avail: f64,
    to_us_msat: u64,
    total_msat: u64,
    graph_max_chan_side_msat: u64,
) -> Result<Summary, Error> {
    let statestr = ShortChannelState(chan.state.unwrap());

    let scidsortdummy = ShortChannelId::from_str("999999999x9999x99").unwrap();
    let scid = match chan.short_channel_id {
        Some(scid) => scid,
        None => scidsortdummy,
    };

    Ok(Summary {
        graph_sats: draw_chans_graph(config, total_msat, to_us_msat, graph_max_chan_side_msat),
        out_sats: to_us_msat / 1_000,
        in_sats: (total_msat - to_us_msat) / 1_000,
        scid_raw: scid,
        scid: if scidsortdummy == scid {
            "PENDING".to_string()
        } else {
            scid.to_string()
        },
        max_htlc: Amount::msat(&chan.maximum_htlc_out_msat.unwrap()) / 1_000,
        flag: make_channel_flags(chan.private, chan.peer_connected.unwrap()),
        base: Amount::msat(&chan.fee_base_msat.unwrap()),
        ppm: chan.fee_proportional_millionths.unwrap(),
        alias: if config.utf8.value {
            alias.to_string()
        } else {
            alias.replace(|c: char| !c.is_ascii(), "?")
        },
        peer_id: chan.peer_id.unwrap(),
        uptime: avail * 100.0,
        htlcs: chan.htlcs.clone().unwrap_or_default().len(),
        state: statestr.to_string(),
    })
}

fn sort_summary(config: &Config, table: &mut [Summary]) {
    match config.sort_by.value.clone() {
        col if col.eq("OUT_SATS") => table.sort_by_key(|x| x.out_sats),
        col if col.eq("IN_SATS") => table.sort_by_key(|x| x.in_sats),
        col if col.eq("SCID_RAW") => table.sort_by_key(|x| x.scid_raw),
        col if col.eq("SCID") => table.sort_by_key(|x| x.scid_raw),
        col if col.eq("MAX_HTLC") => table.sort_by_key(|x| x.max_htlc),
        col if col.eq("FLAG") => table.sort_by_key(|x| x.flag.clone()),
        col if col.eq("BASE") => table.sort_by_key(|x| x.base),
        col if col.eq("PPM") => table.sort_by_key(|x| x.ppm),
        col if col.eq("ALIAS") => table.sort_by_key(|x| {
            x.alias
                .chars()
                .filter(|c| c.is_ascii() && !c.is_whitespace() && c != &'@')
                .collect::<String>()
                .to_ascii_lowercase()
        }),
        col if col.eq("UPTIME") => table.sort_by(|x, y| {
            x.uptime
                .partial_cmp(&y.uptime)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        col if col.eq("PEER_ID") => table.sort_by_key(|x| x.peer_id),
        col if col.eq("HTLCS") => table.sort_by_key(|x| x.htlcs),
        col if col.eq("STATE") => table.sort_by_key(|x| x.state.clone()),
        _ => table.sort_by_key(|x| x.scid_raw),
    }
}

fn format_summary(config: &Config, sumtable: &mut Table) {
    config.style.value.apply(sumtable);
    for head in Summary::FIELD_NAMES_AS_ARRAY {
        if !config.columns.value.contains(&head.to_string()) {
            sumtable.with(Disable::column(ByColumnName::new(head)));
        }
    }
    sumtable.with(
        Modify::new(ByColumnName::new("ALIAS"))
            .with(Width::truncate(config.max_alias_length.value as usize).suffix("[..]")),
    );
    sumtable.with(Modify::new(ByColumnName::new("OUT_SATS")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("IN_SATS")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("MAX_HTLC")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("FLAG")).with(Alignment::center()));
    sumtable.with(Modify::new(ByColumnName::new("BASE")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("PPM")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("UPTIME")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("HTLCS")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("STATE")).with(Alignment::center()));

    sumtable.with(
        Modify::new(ByColumnName::new("UPTIME").not(Rows::first())).with(Format::content(|s| {
            let av = s.parse::<f64>().unwrap_or(-1.0);
            if av < 0.0 {
                "N/A".to_string()
            } else {
                format!("{}%", av.round())
            }
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("OUT_SATS").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("IN_SATS").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("MAX_HTLC").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("BASE").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("PPM").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );

    sumtable.with(Modify::new(Rows::first()).with(Alignment::center()));
}

fn get_addrstr(getinfo: &GetinfoResponse) -> String {
    let mut address = None;
    if let Some(addr) = &getinfo.address {
        if !addr.is_empty() {
            if addr
                .iter()
                .any(|x| matches!(x.item_type, GetinfoAddressType::IPV4))
            {
                address = Some(
                    addr.iter()
                        .find(|x| matches!(x.item_type, GetinfoAddressType::IPV4))
                        .unwrap()
                        .clone(),
                )
            } else {
                address = Some(addr.first().unwrap().clone())
            }
        }
    }
    let mut bindaddr = None;
    if address.is_none() {
        if let Some(bind) = &getinfo.binding {
            if !bind.is_empty() {
                bindaddr = Some(bind.first().unwrap().clone())
            }
        }
    }
    match address {
        Some(a) => {
            getinfo.id.to_string()
                + "@"
                + &a.address.unwrap_or("missing address".to_string())
                + ":"
                + &a.port.to_string()
        }
        None => match bindaddr {
            Some(baddr) => {
                getinfo.id.to_string()
                    + "@"
                    + &baddr.address.unwrap_or("missing address".to_string())
                    + ":"
                    + &baddr.port.unwrap_or(9735).to_string()
            }
            None => "No addresses found!".to_string(),
        },
    }
}
