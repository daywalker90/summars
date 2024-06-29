use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::primitives::ShortChannelId;
use cln_rpc::ClnRpc;
use cln_rpc::{
    model::requests::*,
    model::responses::*,
    primitives::{Amount, PublicKey},
};

use log::debug;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::str::FromStr;
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::grid::records::vec_records::Cell;
use tabled::grid::records::Records;
use tabled::settings::location::{ByColumnName, Locator};
use tabled::settings::object::{Object, Rows};
use tabled::settings::{Alignment, Disable, Format, Modify, Panel, Width};

use serde_json::json;

use tabled::Table;
use tokio::time::Instant;

use crate::config::validateargs;
use crate::structs::{
    ChannelVisibility, Config, ConnectionStatus, Forwards, ForwardsFilterStats, GraphCharset,
    Invoices, InvoicesFilterStats, PagingIndex, Pays, PluginState, ShortChannelState, Summary,
    NODE_GOSSIP_MISS, NO_ALIAS_SET,
};
use crate::util::{
    draw_chans_graph, hex_encode, is_active_state, make_channel_flags, make_rpc_path, sort_columns,
    timestamp_to_localized_datetime_string, u64_to_btc_string, u64_to_sat_string,
};

pub async fn summary(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let now = Instant::now();

    let rpc_path = make_rpc_path(&p);
    let mut rpc = ClnRpc::new(&rpc_path).await?;

    let mut config = p.state().config.lock().clone();
    validateargs(v, &mut config)?;

    let getinfo = rpc.call_typed(&GetinfoRequest {}).await?;
    debug!(
        "Getinfo. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let peers = rpc
        .call_typed(&ListpeersRequest {
            id: None,
            level: None,
        })
        .await?
        .peers;
    debug!(
        "Listpeers. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    let peer_channels = rpc
        .call_typed(&ListpeerchannelsRequest { id: None })
        .await?
        .channels;
    debug!(
        "Listpeerchannels. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let funds = rpc
        .call_typed(&ListfundsRequest { spent: Some(false) })
        .await?;
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
            .channel_states
            .contains(&ShortChannelState(chan.state))
            || if let Some(excl_vis) = &config.exclude_channel_states.value.channel_visibility {
                match excl_vis {
                    ChannelVisibility::Private => chan.private.unwrap(),
                    ChannelVisibility::Public => !chan.private.unwrap(),
                }
            } else {
                false
            }
            || if let Some(excl_conn) = &config.exclude_channel_states.value.connection_status {
                match excl_conn {
                    ConnectionStatus::Online => chan.peer_connected,
                    ConnectionStatus::Offline => !chan.peer_connected,
                }
            } else {
                false
            }
        {
            filter_count += 1;
            continue;
        }
        let alias = get_alias(&mut rpc, p.clone(), chan.peer_id).await?;

        let to_us_msat = Amount::msat(
            &chan
                .to_us_msat
                .ok_or(anyhow!("Channel with {} has no msats to us!", chan.peer_id))?,
        );
        let total_msat = Amount::msat(&chan.total_msat.ok_or(anyhow!(
            "Channel with {} has no total amount!",
            chan.peer_id
        ))?);
        let our_reserve = Amount::msat(
            &chan
                .our_reserve_msat
                .ok_or(anyhow!("Channel with {} has no our_reserve!", chan.peer_id))?,
        );
        let their_reserve = Amount::msat(&chan.their_reserve_msat.ok_or(anyhow!(
            "Channel with {} has no their_reserve!",
            chan.peer_id
        ))?);

        if matches!(
            chan.state,
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

        let avail = match p.state().avail.lock().get(&chan.peer_id) {
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
            if chan.peer_connected {
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

    let forwards;
    let forwards_filter_stats;
    if config.forwards.value > 0 {
        (forwards, forwards_filter_stats) =
            recent_forwards(&mut rpc, &peer_channels, p.clone(), &config, now).await?;
        debug!(
            "End of forwards table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        forwards = Vec::new();
        forwards_filter_stats = ForwardsFilterStats::default();
    }

    let pays;
    if config.pays.value > 0 {
        pays = recent_pays(&mut rpc, p.clone(), &config, now, getinfo.id).await?;
        debug!(
            "End of pays table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        pays = Vec::new();
    }

    let invoices;
    let invoices_filter_stats;
    if config.invoices.value > 0 {
        (invoices, invoices_filter_stats) =
            recent_invoices(p.clone(), &mut rpc, &config, now).await?;
        debug!(
            "End of invoices table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        invoices = Vec::new();
        invoices_filter_stats = InvoicesFilterStats::default();
    }

    let addr_str = get_addrstr(&getinfo);

    if config.json.value {
        Ok(json!({"info":{
            "address":addr_str,
            "num_utxos":funds.outputs.len(),
            "utxo_amount": format!("{} BTC",u64_to_btc_string(&config, utxo_amt)?),
            "num_channels":channel_count,
            "num_connected":num_connected,
            "num_gossipers":num_gossipers,
            "avail_out":format!("{} BTC",u64_to_btc_string(&config, avail_out)?),
            "avail_in":format!("{} BTC",u64_to_btc_string(&config, avail_in)?),
            "fees_collected":format!("{} BTC",u64_to_btc_string(&config, Amount::msat(&getinfo.fees_collected_msat))?),
        },
        "channels":table,
        "forwards":forwards,
        "pays":pays,
        "invoices":invoices}))
    } else {
        let mut sumtable = Table::new(table);
        format_summary(&config, &mut sumtable)?;
        draw_graph_sats_name(&config, &mut sumtable, graph_max_chan_side_msat)?;
        debug!(
            "Format summary. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );

        if filter_count > 0 {
            sumtable.with(Panel::footer(format!(
                "\n {} channel{} filtered.",
                filter_count,
                if filter_count == 1 { "" } else { "s" }
            )));
            sumtable.with(Modify::new(Rows::last()).with(Alignment::left()));
        }

        let mut result = sumtable.to_string();
        if config.forwards.value > 0 {
            result +=
                &("\n\n".to_owned() + &format_forwards(forwards, &config, forwards_filter_stats)?);
        }
        if config.pays.value > 0 {
            result += &("\n\n".to_owned() + &format_pays(pays, &config)?);
        }
        if config.invoices.value > 0 {
            result +=
                &("\n\n".to_owned() + &format_invoices(invoices, &config, invoices_filter_stats)?);
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
}

async fn recent_forwards(
    rpc: &mut ClnRpc,
    peer_channels: &[ListpeerchannelsChannels],
    plugin: Plugin<PluginState>,
    config: &Config,
    now: Instant,
) -> Result<(Vec<Forwards>, ForwardsFilterStats), Error> {
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
    let forwards = rpc
        .call_typed(&ListforwardsRequest {
            status: Some(ListforwardsStatus::SETTLED),
            in_channel: None,
            out_channel: None,
            index: Some(ListforwardsIndex::CREATED),
            start: Some(fw_index.start),
            limit: None,
        })
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
            let inchan = config
                .forwards_alias
                .value
                .then(|| {
                    chanmap.get(&forward.in_channel).and_then(|chan| {
                        alias_map
                            .get::<PublicKey>(&chan.peer_id)
                            .filter(|alias| alias.as_str() != (NO_ALIAS_SET))
                            .cloned()
                    })
                })
                .flatten()
                .unwrap_or_else(|| forward.in_channel.to_string());

            let fw_outchan = forward.out_channel.unwrap();
            let outchan = config
                .forwards_alias
                .value
                .then(|| {
                    chanmap.get(&fw_outchan).and_then(|chan| {
                        alias_map
                            .get::<PublicKey>(&chan.peer_id)
                            .filter(|alias| alias.as_str() != (NO_ALIAS_SET))
                            .cloned()
                    })
                })
                .flatten()
                .unwrap_or_else(|| fw_outchan.to_string());

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
                    received_time: (forward.received_time * 1_000.0) as u64,
                    received_time_str: timestamp_to_localized_datetime_string(
                        config,
                        forward.received_time as u64,
                    )?,
                    resolved_time: (forward.resolved_time.unwrap() * 1_000.0) as u64,
                    resolved_time_str: timestamp_to_localized_datetime_string(
                        config,
                        forward.resolved_time.unwrap() as u64,
                    )?,
                    in_channel_alias: if config.utf8.value {
                        inchan
                    } else {
                        inchan.replace(|c: char| !c.is_ascii(), "?")
                    },
                    in_channel: forward.in_channel,
                    out_channel_alias: if config.utf8.value {
                        outchan
                    } else {
                        outchan.replace(|c: char| !c.is_ascii(), "?")
                    },
                    out_channel: forward.out_channel.unwrap(),
                    in_msats: Amount::msat(&forward.in_msat),
                    out_msats: Amount::msat(&forward.out_msat.unwrap()),
                    fee_msats: Amount::msat(&forward.fee_msat.unwrap()),
                    in_sats: ((Amount::msat(&forward.in_msat) as f64) / 1_000.0).round() as u64,
                    out_sats: ((Amount::msat(&forward.out_msat.unwrap()) as f64) / 1_000.0).round()
                        as u64,
                    fee_sats: ((Amount::msat(&forward.fee_msat.unwrap()) as f64) / 1_000.0).round()
                        as u64,
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
    table.sort_by_key(|x| x.resolved_time);
    Ok((
        table,
        ForwardsFilterStats {
            filter_amt_sum_msat,
            filter_fee_sum_msat,
            filter_count,
        },
    ))
}

fn format_forwards(
    table: Vec<Forwards>,
    config: &Config,
    filter_stats: ForwardsFilterStats,
) -> Result<String, Error> {
    let mut fwtable = Table::new(table);
    config.flow_style.value.apply(&mut fwtable);
    for head in Forwards::FIELD_NAMES_AS_ARRAY {
        if !config.forwards_columns.value.contains(&head.to_string()) {
            fwtable.with(Disable::column(ByColumnName::new(head)));
        }
    }
    let headers = fwtable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_string())
        .collect::<Vec<String>>();
    let records = fwtable.get_records_mut();
    if headers.len() != config.forwards_columns.value.len() {
        return Err(anyhow!(
            "Error formatting forwards! Length difference detected: {} {}",
            headers.join(","),
            config.forwards_columns.value.join(",")
        ));
    }
    sort_columns(records, &headers, &config.forwards_columns.value);

    if config.max_alias_length.value < 0 {
        fwtable.with(
            Modify::new(ByColumnName::new("in_channel")).with(
                Width::wrap(config.max_alias_length.value.unsigned_abs() as usize).keep_words(),
            ),
        );
    } else {
        fwtable.with(
            Modify::new(ByColumnName::new("in_channel"))
                .with(Width::truncate(config.max_alias_length.value as usize).suffix("[..]")),
        );
    }

    if config.max_alias_length.value < 0 {
        fwtable.with(
            Modify::new(ByColumnName::new("out_channel")).with(
                Width::wrap(config.max_alias_length.value.unsigned_abs() as usize).keep_words(),
            ),
        );
    } else {
        fwtable.with(
            Modify::new(ByColumnName::new("out_channel"))
                .with(Width::truncate(config.max_alias_length.value as usize).suffix("[..]")),
        );
    }

    fwtable.with(Modify::new(ByColumnName::new("in_sats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("in_sats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("in_msats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("in_msats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("out_sats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("out_sats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("out_msats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("out_msats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("fee_sats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("fee_sats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("fee_msats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("fee_msats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );

    fwtable.with(Panel::header("forwards"));
    fwtable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if filter_stats.filter_count > 0 {
        let filter_sum_result = format!(
            "\nFiltered {} forward{} with {} sats routed and {} msat fees.",
            filter_stats.filter_count,
            if filter_stats.filter_count == 1 {
                ""
            } else {
                "s"
            },
            u64_to_sat_string(
                config,
                ((filter_stats.filter_amt_sum_msat as f64) / 1_000.0).round() as u64
            )?,
            u64_to_sat_string(config, filter_stats.filter_fee_sum_msat)?,
        );
        fwtable.with(Panel::footer(filter_sum_result));
    }
    Ok(fwtable.to_string())
}

async fn recent_pays(
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
    config: &Config,
    now: Instant,
    mypubkey: PublicKey,
) -> Result<Vec<Pays>, Error> {
    let pays = rpc
        .call_typed(&ListpaysRequest {
            bolt11: None,
            payment_hash: None,
            status: Some(ListpaysStatus::COMPLETE),
        })
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
            let destination = get_alias(rpc, plugin.clone(), pay.destination.unwrap()).await?;
            table.push(Pays {
                completed_at: pay.completed_at.unwrap(),
                completed_at_str: timestamp_to_localized_datetime_string(
                    config,
                    pay.completed_at.unwrap(),
                )?,
                payment_hash: pay.payment_hash.to_string(),
                msats_sent: Amount::msat(&pay.amount_sent_msat.unwrap()),
                sats_sent: ((Amount::msat(&pay.amount_sent_msat.unwrap()) as f64) / 1_000.0).round()
                    as u64,
                destination: if destination == NODE_GOSSIP_MISS {
                    pay.destination.unwrap().to_string()
                } else if config.utf8.value {
                    destination
                } else {
                    destination.replace(|c: char| !c.is_ascii(), "?")
                },
                description: if config
                    .pays_columns
                    .value
                    .contains(&"description".to_string())
                    && !config.json.value
                {
                    if let Some(desc) = pay.description {
                        desc
                    } else if let Some(b11) = pay.bolt11 {
                        let invoice = rpc.call_typed(&DecodeRequest { string: b11 }).await?;
                        invoice.description.unwrap_or_default()
                    } else {
                        let b12 = pay
                            .bolt12
                            .ok_or_else(|| anyhow!("No description, bolt11 or bolt12 found"))?;
                        let invoice = rpc.call_typed(&DecodeRequest { string: b12 }).await?;
                        invoice.offer_description.unwrap_or_default()
                    }
                } else {
                    String::new()
                },
                preimage: hex_encode(&pay.preimage.unwrap().to_vec()),
                msats_requested: Amount::msat(&pay.amount_msat.unwrap()),
                sats_requested: ((Amount::msat(&pay.amount_msat.unwrap()) as f64) / 1_000.0).round()
                    as u64,
                fee_msats: pay.amount_sent_msat.unwrap().msat() - pay.amount_msat.unwrap().msat(),
                fee_sats: (((pay.amount_sent_msat.unwrap().msat() - pay.amount_msat.unwrap().msat())
                    as f64)
                    / 1_000.0)
                    .round() as u64,
            })
        }
    }
    debug!(
        "Build pays table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    table.sort_by_key(|x| x.completed_at);
    Ok(table)
}

fn format_pays(table: Vec<Pays>, config: &Config) -> Result<String, Error> {
    let mut paystable = Table::new(table);
    config.flow_style.value.apply(&mut paystable);
    for head in Pays::FIELD_NAMES_AS_ARRAY {
        if !config.pays_columns.value.contains(&head.to_string()) {
            paystable.with(Disable::column(ByColumnName::new(head)));
        }
    }
    let headers = paystable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_string())
        .collect::<Vec<String>>();
    let records = paystable.get_records_mut();
    if headers.len() != config.pays_columns.value.len() {
        return Err(anyhow!(
            "Error formatting pays! Length difference detected: {} {}",
            headers.join(","),
            config.pays_columns.value.join(",")
        ));
    }
    sort_columns(records, &headers, &config.pays_columns.value);

    if config.max_alias_length.value < 0 {
        paystable.with(
            Modify::new(ByColumnName::new("destination")).with(
                Width::wrap(config.max_alias_length.value.unsigned_abs() as usize).keep_words(),
            ),
        );
    } else {
        paystable.with(
            Modify::new(ByColumnName::new("destination"))
                .with(Width::truncate(config.max_alias_length.value as usize).suffix("[..]")),
        );
    }

    paystable.with(Modify::new(ByColumnName::new("sats_sent")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("sats_sent").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    paystable.with(Modify::new(ByColumnName::new("msats_sent")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("msats_sent").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );

    paystable.with(Modify::new(ByColumnName::new("sats_requested")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("sats_requested").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );
    paystable.with(Modify::new(ByColumnName::new("msats_requested")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("msats_requested").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );

    paystable.with(Modify::new(ByColumnName::new("fee_sats")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("fee_sats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    paystable.with(Modify::new(ByColumnName::new("fee_msats")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("fee_msats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );

    if config.max_desc_length.value < 0 {
        paystable.with(
            Modify::new(ByColumnName::new("description")).with(
                Width::wrap(config.max_desc_length.value.unsigned_abs() as usize).keep_words(),
            ),
        );
    } else {
        paystable.with(
            Modify::new(ByColumnName::new("description"))
                .with(Width::truncate(config.max_desc_length.value as usize).suffix("[..]")),
        );
    }

    paystable.with(Panel::header("pays"));
    paystable.with(Modify::new(Rows::first()).with(Alignment::center()));

    Ok(paystable.to_string())
}

async fn recent_invoices(
    plugin: Plugin<PluginState>,
    rpc: &mut ClnRpc,
    config: &Config,
    now: Instant,
) -> Result<(Vec<Invoices>, InvoicesFilterStats), Error> {
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
    let invoices = rpc
        .call_typed(&ListinvoicesRequest {
            label: None,
            invstring: None,
            payment_hash: None,
            offer_id: None,
            index: Some(ListinvoicesIndex::CREATED),
            start: Some(inv_index.start),
            limit: None,
        })
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
                        msats_received: Amount::msat(&invoice.amount_received_msat.unwrap()),
                        sats_received: ((Amount::msat(&invoice.amount_received_msat.unwrap())
                            as f64)
                            / 1_000.0)
                            .round() as u64,
                        description: invoice.description.unwrap_or_default(),
                        payment_hash: invoice.payment_hash.to_string(),
                        preimage: hex_encode(&invoice.payment_preimage.unwrap().to_vec()),
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

    Ok((
        table,
        InvoicesFilterStats {
            filter_amt_sum_msat,
            filter_count,
        },
    ))
}

fn format_invoices(
    table: Vec<Invoices>,
    config: &Config,
    filter_stats: InvoicesFilterStats,
) -> Result<String, Error> {
    let mut invoicestable = Table::new(table);
    config.flow_style.value.apply(&mut invoicestable);
    for head in Invoices::FIELD_NAMES_AS_ARRAY {
        if !config.invoices_columns.value.contains(&head.to_string()) {
            invoicestable.with(Disable::column(ByColumnName::new(head)));
        }
    }
    let headers = invoicestable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_string())
        .collect::<Vec<String>>();
    let records = invoicestable.get_records_mut();
    if headers.len() != config.invoices_columns.value.len() {
        return Err(anyhow!(
            "Error formatting invoices! Length difference detected: {} {}",
            headers.join(","),
            config.invoices_columns.value.join(",")
        ));
    }
    sort_columns(records, &headers, &config.invoices_columns.value);

    if config.max_desc_length.value < 0 {
        invoicestable.with(
            Modify::new(ByColumnName::new("description")).with(
                Width::wrap(config.max_desc_length.value.unsigned_abs() as usize).keep_words(),
            ),
        );
    } else {
        invoicestable.with(
            Modify::new(ByColumnName::new("description"))
                .with(Width::truncate(config.max_desc_length.value as usize).suffix("[..]")),
        );
    }

    if config.max_label_length.value < 0 {
        invoicestable.with(
            Modify::new(ByColumnName::new("label")).with(
                Width::wrap(config.max_label_length.value.unsigned_abs() as usize).keep_words(),
            ),
        );
    } else {
        invoicestable.with(
            Modify::new(ByColumnName::new("label"))
                .with(Width::truncate(config.max_label_length.value as usize).suffix("[..]")),
        );
    }

    invoicestable.with(Modify::new(ByColumnName::new("sats_received")).with(Alignment::right()));
    invoicestable.with(
        Modify::new(ByColumnName::new("sats_received").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );
    invoicestable.with(Modify::new(ByColumnName::new("msats_received")).with(Alignment::right()));
    invoicestable.with(
        Modify::new(ByColumnName::new("msats_received").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );

    invoicestable.with(Panel::header("invoices"));
    invoicestable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if filter_stats.filter_count > 0 {
        let filter_sum_result = format!(
            "\nFiltered {} invoice{} with {} sats total.",
            filter_stats.filter_count,
            if filter_stats.filter_count == 1 {
                ""
            } else {
                "s"
            },
            u64_to_sat_string(
                config,
                ((filter_stats.filter_amt_sum_msat as f64) / 1_000.0).round() as u64
            )?
        );
        invoicestable.with(Panel::footer(filter_sum_result));
    }
    Ok(invoicestable.to_string())
}

async fn get_alias(
    rpc: &mut ClnRpc,
    p: Plugin<PluginState>,
    peer_id: PublicKey,
) -> Result<String, Error> {
    let alias_map = p.state().alias_map.lock().clone();
    let alias;
    match alias_map.get::<PublicKey>(&peer_id) {
        Some(a) => alias = a.clone(),
        None => match rpc
            .call_typed(&ListnodesRequest { id: Some(peer_id) })
            .await?
            .nodes
            .first()
        {
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
    let statestr = ShortChannelState(chan.state);

    let scidsortdummy = ShortChannelId::from_str("999999999x9999x99").unwrap();
    let scid = match chan.short_channel_id {
        Some(scid) => scid,
        None => scidsortdummy,
    };

    Ok(Summary {
        graph_sats: draw_chans_graph(config, total_msat, to_us_msat, graph_max_chan_side_msat),
        out_sats: ((to_us_msat as f64) / 1_000.0).round() as u64,
        in_sats: (((total_msat - to_us_msat) as f64) / 1_000.0).round() as u64,
        scid_raw: scid,
        scid: if scidsortdummy == scid {
            "PENDING".to_string()
        } else {
            scid.to_string()
        },
        max_htlc: ((Amount::msat(&chan.maximum_htlc_out_msat.unwrap()) as f64) / 1_000.0).round()
            as u64,
        flag: make_channel_flags(chan.private.unwrap(), !chan.peer_connected),
        private: chan.private.unwrap(),
        offline: !chan.peer_connected,
        base: Amount::msat(&chan.fee_base_msat.unwrap()),
        ppm: chan.fee_proportional_millionths.unwrap(),
        alias: if config.utf8.value {
            alias.to_string()
        } else {
            alias.replace(|c: char| !c.is_ascii(), "?")
        },
        peer_id: chan.peer_id,
        uptime: avail * 100.0,
        htlcs: chan.htlcs.clone().unwrap_or_default().len(),
        state: statestr.to_string(),
    })
}

fn sort_summary(config: &Config, table: &mut [Summary]) {
    let reverse = config.sort_by.value.starts_with('-');
    let sort_by = if reverse {
        &config.sort_by.value[1..]
    } else {
        &config.sort_by.value
    };
    match sort_by {
        col if col.eq("OUT_SATS") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.out_sats))
            } else {
                table.sort_by_key(|x| x.out_sats)
            }
        }
        col if col.eq("IN_SATS") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.in_sats))
            } else {
                table.sort_by_key(|x| x.in_sats)
            }
        }
        col if col.eq("MAX_HTLC") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.max_htlc))
            } else {
                table.sort_by_key(|x| x.max_htlc)
            }
        }
        col if col.eq("FLAG") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.flag.clone()))
            } else {
                table.sort_by_key(|x| x.flag.clone())
            }
        }
        col if col.eq("BASE") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.base))
            } else {
                table.sort_by_key(|x| x.base)
            }
        }
        col if col.eq("PPM") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.ppm))
            } else {
                table.sort_by_key(|x| x.ppm)
            }
        }
        col if col.eq("ALIAS") => {
            if reverse {
                table.sort_by_key(|x| {
                    Reverse(
                        x.alias
                            .chars()
                            .filter(|c| c.is_ascii() && !c.is_whitespace() && c != &'@')
                            .collect::<String>()
                            .to_ascii_lowercase(),
                    )
                })
            } else {
                table.sort_by_key(|x| {
                    x.alias
                        .chars()
                        .filter(|c| c.is_ascii() && !c.is_whitespace() && c != &'@')
                        .collect::<String>()
                        .to_ascii_lowercase()
                })
            }
        }
        col if col.eq("UPTIME") => {
            if reverse {
                table.sort_by(|x, y| {
                    y.uptime
                        .partial_cmp(&x.uptime)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            } else {
                table.sort_by(|x, y| {
                    x.uptime
                        .partial_cmp(&y.uptime)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
        }
        col if col.eq("PEER_ID") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.peer_id))
            } else {
                table.sort_by_key(|x| x.peer_id)
            }
        }
        col if col.eq("HTLCS") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.htlcs))
            } else {
                table.sort_by_key(|x| x.htlcs)
            }
        }
        col if col.eq("STATE") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.state.clone()))
            } else {
                table.sort_by_key(|x| x.state.clone())
            }
        }
        _ => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.scid_raw))
            } else {
                table.sort_by_key(|x| x.scid_raw)
            }
        }
    }
}

fn format_summary(config: &Config, sumtable: &mut Table) -> Result<(), Error> {
    config.style.value.apply(sumtable);
    for head in Summary::FIELD_NAMES_AS_ARRAY {
        if !config.columns.value.contains(&head.to_string()) {
            sumtable.with(Disable::column(ByColumnName::new(
                head.to_ascii_uppercase(),
            )));
        }
    }

    let headers = sumtable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_string())
        .collect::<Vec<String>>();
    let records = sumtable.get_records_mut();
    if headers.len() != config.columns.value.len() {
        return Err(anyhow!(
            "Error formatting channels! Length difference detected: {} {}",
            headers.join(","),
            config.columns.value.join(",")
        ));
    }
    sort_columns(records, &headers, &config.columns.value);

    if config.max_alias_length.value < 0 {
        sumtable.with(
            Modify::new(ByColumnName::new("ALIAS")).with(
                Width::wrap(config.max_alias_length.value.unsigned_abs() as usize).keep_words(),
            ),
        );
    } else {
        sumtable.with(
            Modify::new(ByColumnName::new("ALIAS"))
                .with(Width::truncate(config.max_alias_length.value as usize).suffix("[..]")),
        );
    }

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
    Ok(())
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

fn draw_graph_sats_name(
    config: &Config,
    sumtable: &mut Table,
    graph_max_chan_side_msat: u64,
) -> Result<(), Error> {
    let draw_utf8 = GraphCharset::new_utf8();
    let draw_ascii = GraphCharset::new_ascii();
    let draw = if config.utf8.value {
        &draw_utf8
    } else {
        &draw_ascii
    };
    let btc_str = u64_to_btc_string(config, graph_max_chan_side_msat)?;
    sumtable.with(
        Modify::new(
            ByColumnName::new("GRAPH_SATS").intersect(Locator::by(|n| n.contains("GRAPH_SATS"))),
        )
        .with(format!(
            "{}{:<12} OUT GRAPH_SATS IN {:>14}{}",
            draw.left, btc_str, btc_str, draw.right
        )),
    );
    Ok(())
}
