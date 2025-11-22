use std::{
    cmp::Reverse,
    collections::HashMap,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, Error};
use cln_plugin::Plugin;
use cln_rpc::{
    model::{
        requests::{
            GetinfoRequest,
            ListchannelsRequest,
            ListfundsRequest,
            ListpeerchannelsRequest,
            ListpeersRequest,
            PingRequest,
        },
        responses::{
            GetinfoAddressType,
            GetinfoResponse,
            ListfundsOutputsStatus,
            ListpeerchannelsChannels,
        },
    },
    primitives::{Amount, ChannelState, PublicKey, ShortChannelId},
    ClnRpc,
};
use serde_json::json;
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::{
    grid::records::{vec_records::Cell, Records},
    settings::{
        location::{ByColumnName, Locator},
        object::{Object, Rows},
        Alignment,
        Format,
        Modify,
        Panel,
        Remove,
        Width,
    },
    Table,
};
use tokio::{
    sync::Semaphore,
    time::{timeout, Instant},
};

use crate::{
    config::validateargs,
    forwards::{format_forwards, recent_forwards},
    invoices::{format_invoices, recent_invoices},
    pays::{format_pays, recent_pays},
    structs::{
        ChannelVisibility,
        Config,
        ConnectionStatus,
        ForwardsFilterStats,
        GraphCharset,
        InvoicesFilterStats,
        PluginState,
        ShortChannelState,
        Summary,
        Totals,
    },
    util::{
        at_or_above_version,
        draw_chans_graph,
        get_alias,
        is_active_state,
        make_channel_flags,
        make_rpc_path,
        sort_columns,
        u64_to_btc_string,
        u64_to_sat_string,
    },
};

const PING_TIMEOUT_MS: u64 = 5000;

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
    log::debug!("Getinfo. Total: {}ms", now.elapsed().as_millis());

    let peers = rpc
        .call_typed(&ListpeersRequest {
            id: None,
            level: None,
        })
        .await?
        .peers;
    log::debug!("Listpeers. Total: {}ms", now.elapsed().as_millis());
    let peer_channels = rpc
        .call_typed(&ListpeerchannelsRequest {
            id: None,
            short_channel_id: None,
        })
        .await?
        .channels;
    log::debug!("Listpeerchannels. Total: {}ms", now.elapsed().as_millis());

    let funds = rpc
        .call_typed(&ListfundsRequest { spent: Some(false) })
        .await?;
    log::debug!("Listfunds. Total: {}ms", now.elapsed().as_millis());

    let mut utxo_amt: u64 = 0;
    for utxo in &funds.outputs {
        if let ListfundsOutputsStatus::CONFIRMED = utxo.status {
            utxo_amt += Amount::msat(&utxo.amount_msat);
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

    let mut table = HashMap::with_capacity(peer_channels.len());

    let num_gossipers = peers
        .iter()
        .filter(|s| s.num_channels.unwrap() == 0)
        .count();

    for (id, chan) in peer_channels.iter().enumerate() {
        if config
            .exclude_channel_states
            .channel_states
            .contains(&ShortChannelState(chan.state))
            || if let Some(excl_vis) = &config.exclude_channel_states.channel_visibility {
                match excl_vis {
                    ChannelVisibility::Private => chan.private.unwrap(),
                    ChannelVisibility::Public => !chan.private.unwrap(),
                }
            } else {
                false
            }
            || if let Some(excl_conn) = &config.exclude_channel_states.connection_status {
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
            ChannelState::CHANNELD_NORMAL | ChannelState::CHANNELD_AWAITING_SPLICE
        ) {
            if our_reserve < to_us_msat {
                avail_out += to_us_msat - our_reserve;
            }
            if their_reserve < total_msat - to_us_msat {
                avail_in += total_msat - to_us_msat - their_reserve;
            }
        }

        let avail = match p.state().avail.lock().get(&chan.peer_id) {
            Some(a) => a.avail,
            None => -1.0,
        };

        let summary = chan_to_summary(
            &rpc_path,
            &config,
            &getinfo.version,
            chan,
            alias,
            avail,
            graph_max_chan_side_msat,
        )
        .await?;
        table.insert(id, summary);

        if is_active_state(chan) {
            if chan.peer_connected {
                num_connected += 1;
            }
            channel_count += 1;
        }
    }
    log::debug!("First summary-loop. Total: {}ms", now.elapsed().as_millis());

    get_pings(rpc_path, &config, &getinfo.version, &mut table).await?;
    log::debug!("Got pings. Total: {}ms", now.elapsed().as_millis());

    let mut table = table.into_values().collect::<Vec<Summary>>();

    sort_summary(&config, &mut table);
    log::debug!("Sort summary. Total: {}ms", now.elapsed().as_millis());

    let mut totals = Totals {
        pays_amount_msat: None,
        pays_amount_sent_msat: None,
        pays_fees_msat: None,
        invoices_amount_received_msat: None,
        forwards_amount_in_msat: None,
        forwards_amount_out_msat: None,
        forwards_fees_msat: None,
    };

    let forwards;
    let forwards_filter_stats;
    if config.forwards > 0 {
        (forwards, forwards_filter_stats) = recent_forwards(
            &mut rpc,
            &peer_channels,
            p.clone(),
            &config,
            &mut totals,
            now,
        )
        .await?;
        log::debug!(
            "End of forwards table. Total: {}ms",
            now.elapsed().as_millis()
        );
    } else {
        forwards = Vec::new();
        forwards_filter_stats = ForwardsFilterStats::default();
    }

    let pays;
    if config.pays > 0 {
        pays = recent_pays(
            &mut rpc,
            p.clone(),
            &config,
            &peer_channels,
            &mut totals,
            now,
            &getinfo,
        )
        .await?;
        log::debug!("End of pays table. Total: {}ms", now.elapsed().as_millis());
    } else {
        pays = Vec::new();
    }

    let invoices;
    let invoices_filter_stats;
    if config.invoices > 0 {
        (invoices, invoices_filter_stats) =
            recent_invoices(p.clone(), &mut rpc, &config, &mut totals, now).await?;
        log::debug!(
            "End of invoices table. Total: {}ms",
            now.elapsed().as_millis()
        );
    } else {
        invoices = Vec::new();
        invoices_filter_stats = InvoicesFilterStats::default();
    }

    let addr_str = get_addrstr(&getinfo);

    if config.json {
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
        "invoices":invoices,
        "totals":totals}))
    } else {
        let mut sumtable = Table::new(table);
        format_summary(&config, &mut sumtable)?;
        draw_graph_sats_name(&config, &mut sumtable, graph_max_chan_side_msat)?;
        log::debug!("Format summary. Total: {}ms", now.elapsed().as_millis());

        if filter_count > 0 {
            sumtable.with(Panel::footer(format!(
                "\n {} channel{} filtered.",
                filter_count,
                if filter_count == 1 { "" } else { "s" }
            )));
            sumtable.with(Modify::new(Rows::last()).with(Alignment::left()));
        }

        let mut result = sumtable.to_string();
        if config.forwards > 0 {
            result += &("\n\n".to_owned()
                + &format_forwards(forwards, &config, &totals, &forwards_filter_stats)?);
        }
        log::debug!("Format forwards. Total: {}ms", now.elapsed().as_millis());
        if config.pays > 0 {
            result += &("\n\n".to_owned() + &format_pays(pays, &config, &totals)?);
        }
        log::debug!("Format pays. Total: {}ms", now.elapsed().as_millis());
        if config.invoices > 0 {
            result += &("\n\n".to_owned()
                + &format_invoices(invoices, &config, &totals, &invoices_filter_stats)?);
        }
        log::debug!("Format invoices. Total: {}ms", now.elapsed().as_millis());

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

async fn chan_to_summary(
    rpc_path: &PathBuf,
    config: &Config,
    version: &str,
    chan: &ListpeerchannelsChannels,
    alias: String,
    avail: f64,
    graph_max_chan_side_msat: u64,
) -> Result<Summary, Error> {
    let statestr = ShortChannelState(chan.state);

    let scidsortdummy = ShortChannelId::from_str("999999999x9999x99").unwrap();
    let scid = match chan.short_channel_id {
        Some(scid) => scid,
        None => scidsortdummy,
    };

    let to_us_msat = Amount::msat(
        &chan
            .to_us_msat
            .ok_or(anyhow!("Channel with {} has no msats to us!", chan.peer_id))?,
    );
    let total_msat = Amount::msat(&chan.total_msat.ok_or(anyhow!(
        "Channel with {} has no total amount!",
        chan.peer_id
    ))?);

    let mut in_base = "N/A".to_owned();
    let mut in_ppm = "N/A".to_owned();
    if config.columns.contains(&"in_base".to_owned())
        || config.columns.contains(&"in_ppm".to_owned())
        || config.sort_by.eq_ignore_ascii_case("in_base")
        || config.sort_by.eq_ignore_ascii_case("in_ppm")
    {
        if at_or_above_version(version, "24.02")? {
            if let Some(upd) = &chan.updates {
                if let Some(rem) = &upd.remote {
                    in_base = rem.fee_base_msat.msat().to_string();
                    in_ppm = rem.fee_proportional_millionths.to_string();
                }
            }
        } else if let Some(scid) = chan.short_channel_id {
            let mut rpc = ClnRpc::new(&rpc_path).await?;
            let mut chan_gossip = rpc
                .call_typed(&ListchannelsRequest {
                    destination: None,
                    short_channel_id: Some(scid),
                    source: None,
                })
                .await?
                .channels;
            chan_gossip.retain(|x| x.source == chan.peer_id);
            if let Some(their_goss) = chan_gossip.first() {
                in_base = their_goss.base_fee_millisatoshi.to_string();
                in_ppm = their_goss.fee_per_millionth.to_string();
            }
        }
    }

    let graph_sats = if config.columns.contains(&"graph_sats".to_owned()) {
        draw_chans_graph(config, total_msat, to_us_msat, graph_max_chan_side_msat)
    } else {
        String::new()
    };

    Ok(Summary {
        graph_sats,
        out_sats: ((to_us_msat as f64) / 1_000.0).round() as u64,
        in_sats: (((total_msat - to_us_msat) as f64) / 1_000.0).round() as u64,
        total_sats: ((total_msat as f64) / 1_000.0).round() as u64,
        scid_raw: scid,
        scid: if scidsortdummy == scid {
            "PENDING".to_owned()
        } else {
            scid.to_string()
        },
        min_htlc: ((Amount::msat(&chan.minimum_htlc_out_msat.unwrap()) as f64) / 1_000.0).round()
            as u64,
        max_htlc: ((Amount::msat(&chan.maximum_htlc_out_msat.unwrap()) as f64) / 1_000.0).round()
            as u64,
        flag: make_channel_flags(chan.private.unwrap(), !chan.peer_connected),
        private: chan.private.unwrap(),
        offline: !chan.peer_connected,
        base: Amount::msat(&chan.fee_base_msat.unwrap()),
        in_base,
        ppm: chan.fee_proportional_millionths.unwrap(),
        in_ppm,
        alias: if config.utf8 {
            alias
        } else {
            alias.replace(|c: char| !c.is_ascii(), "?")
        },
        peer_id: chan.peer_id,
        uptime: avail * 100.0,
        htlcs: chan.htlcs.as_ref().map_or(0, Vec::len),
        state: statestr.to_string(),
        perc_us: (to_us_msat as f64 / total_msat as f64) * 100.0,
        ping: 0,
    })
}

async fn get_pings(
    rpc_path: PathBuf,
    config: &Config,
    version: &str,
    table: &mut HashMap<usize, Summary>,
) -> Result<(), Error> {
    if !at_or_above_version(version, "25.09")? {
        log::info!("Not using ping on pre-v25.09 CLN");
        return Ok(());
    } else if config.columns.contains(&"ping".to_owned())
        || config.sort_by.eq_ignore_ascii_case("ping")
    {
        let mut peer_table: HashMap<PublicKey, Vec<usize>> = HashMap::with_capacity(table.len());
        for (id, chan) in table.iter() {
            peer_table.entry(chan.peer_id).or_default().push(*id);
        }
        let concurrency_limit = std::cmp::max(peer_table.len() / 10, 5);
        let semaphore = Arc::new(Semaphore::new(concurrency_limit));
        let mut handles = Vec::new();
        for (peer_id, internal_ids) in peer_table {
            let permit = semaphore.clone().acquire_owned().await?;
            let rpc_path_clone = rpc_path.clone();
            handles.push(tokio::spawn(async move {
                let Ok(mut rpc) = ClnRpc::new(rpc_path_clone).await else {
                    log::warn!("Could not connect to CLN, skipping ping");
                    drop(permit);
                    return (internal_ids, 0);
                };
                log::trace!(
                    "Pinging {}: {}",
                    internal_ids
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect::<Vec<String>>()
                        .join("/"),
                    peer_id
                );
                let now = Instant::now();
                let ping = timeout(
                    Duration::from_millis(PING_TIMEOUT_MS),
                    rpc.call_typed(&PingRequest {
                        len: None,
                        pongbytes: None,
                        id: peer_id,
                    }),
                )
                .await;
                let elapsed = if let Ok(a_p) = ping {
                    if let Ok(_p) = a_p {
                        let elap = now.elapsed().as_millis() as u64;
                        log::trace!("Pinged {peer_id} in {elap}ms");
                        elap
                    } else {
                        log::trace!("Pinging {peer_id} failed");
                        PING_TIMEOUT_MS + 1
                    }
                } else {
                    log::trace!("Pinging {peer_id} timed out");
                    PING_TIMEOUT_MS + 1
                };

                drop(permit);
                (internal_ids, elapsed)
            }));
        }
        for h in handles {
            let result = h.await?;
            for id in result.0 {
                table.get_mut(&id).unwrap().ping = result.1;
            }
        }
    }
    Ok(())
}

fn sort_summary(config: &Config, table: &mut [Summary]) {
    let reverse = config.sort_by.starts_with('-');
    let sort_by = if reverse {
        &config.sort_by[1..]
    } else {
        &config.sort_by
    };
    match sort_by {
        col if col.eq("OUT_SATS") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.out_sats));
            } else {
                table.sort_by_key(|x| x.out_sats);
            }
        }
        col if col.eq("IN_SATS") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.in_sats));
            } else {
                table.sort_by_key(|x| x.in_sats);
            }
        }
        col if col.eq("TOTAL_SATS") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.total_sats));
            } else {
                table.sort_by_key(|x| x.total_sats);
            }
        }
        col if col.eq("MIN_HTLC") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.min_htlc));
            } else {
                table.sort_by_key(|x| x.min_htlc);
            }
        }
        col if col.eq("MAX_HTLC") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.max_htlc));
            } else {
                table.sort_by_key(|x| x.max_htlc);
            }
        }
        col if col.eq("FLAG") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.flag.clone()));
            } else {
                table.sort_by_key(|x| x.flag.clone());
            }
        }
        col if col.eq("BASE") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.base));
            } else {
                table.sort_by_key(|x| x.base);
            }
        }
        col if col.eq("IN_BASE") => {
            if reverse {
                table.sort_by_key(|x| {
                    Reverse(if let Ok(v) = x.in_base.parse::<u64>() {
                        v
                    } else {
                        u64::MAX
                    })
                });
            } else {
                table.sort_by_key(|x| {
                    if let Ok(v) = x.in_base.parse::<u64>() {
                        v
                    } else {
                        u64::MAX
                    }
                });
            }
        }
        col if col.eq("PPM") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.ppm));
            } else {
                table.sort_by_key(|x| x.ppm);
            }
        }
        col if col.eq("IN_PPM") => {
            if reverse {
                table.sort_by_key(|x| {
                    Reverse(if let Ok(v) = x.in_ppm.parse::<u64>() {
                        v
                    } else {
                        u64::MAX
                    })
                });
            } else {
                table.sort_by_key(|x| {
                    if let Ok(v) = x.in_ppm.parse::<u64>() {
                        v
                    } else {
                        u64::MAX
                    }
                });
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
                });
            } else {
                table.sort_by_key(|x| {
                    x.alias
                        .chars()
                        .filter(|c| c.is_ascii() && !c.is_whitespace() && c != &'@')
                        .collect::<String>()
                        .to_ascii_lowercase()
                });
            }
        }
        col if col.eq("UPTIME") => {
            if reverse {
                table.sort_by(|x, y| {
                    y.uptime
                        .partial_cmp(&x.uptime)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            } else {
                table.sort_by(|x, y| {
                    x.uptime
                        .partial_cmp(&y.uptime)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }
        col if col.eq("PEER_ID") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.peer_id));
            } else {
                table.sort_by_key(|x| x.peer_id);
            }
        }
        col if col.eq("HTLCS") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.htlcs));
            } else {
                table.sort_by_key(|x| x.htlcs);
            }
        }
        col if col.eq("STATE") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.state.clone()));
            } else {
                table.sort_by_key(|x| x.state.clone());
            }
        }
        col if col.eq("PERC_US") => {
            if reverse {
                table.sort_by(|x, y| {
                    y.perc_us
                        .partial_cmp(&x.perc_us)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            } else {
                table.sort_by(|x, y| {
                    x.perc_us
                        .partial_cmp(&y.perc_us)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }
        col if col.eq("PING") => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.ping));
            } else {
                table.sort_by_key(|x| x.ping);
            }
        }
        _ => {
            if reverse {
                table.sort_by_key(|x| Reverse(x.scid_raw));
            } else {
                table.sort_by_key(|x| x.scid_raw);
            }
        }
    }
}

fn format_summary(config: &Config, sumtable: &mut Table) -> Result<(), Error> {
    config.style.apply(sumtable);
    for head in Summary::FIELD_NAMES_AS_ARRAY {
        if !config.columns.contains(&head.to_owned()) {
            sumtable.with(Remove::column(ByColumnName::new(head.to_ascii_uppercase())));
        }
    }

    let headers = sumtable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_owned())
        .collect::<Vec<String>>();
    let records = sumtable.get_records_mut();
    if headers.len() != config.columns.len() {
        return Err(anyhow!(
            "Error formatting channels! Length difference detected: {} {}",
            headers.join(","),
            config.columns.join(",")
        ));
    }
    sort_columns(records, &headers, &config.columns);

    if config.max_alias_length < 0 {
        sumtable.with(
            Modify::new(ByColumnName::new("ALIAS")).with(
                Width::wrap(config.max_alias_length.unsigned_abs() as usize).keep_words(true),
            ),
        );
    } else {
        sumtable.with(
            Modify::new(ByColumnName::new("ALIAS"))
                .with(Width::truncate(config.max_alias_length as usize).suffix("[..]")),
        );
    }

    sumtable.with(Modify::new(ByColumnName::new("OUT_SATS")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("IN_SATS")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("TOTAL_SATS")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("MIN_HTLC")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("MAX_HTLC")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("FLAG")).with(Alignment::center()));
    sumtable.with(Modify::new(ByColumnName::new("BASE")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("IN_BASE")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("PPM")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("IN_PPM")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("UPTIME")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("PERC_US")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("HTLCS")).with(Alignment::right()));
    sumtable.with(Modify::new(ByColumnName::new("STATE")).with(Alignment::center()));
    sumtable.with(Modify::new(ByColumnName::new("PING")).with(Alignment::right()));

    sumtable.with(
        Modify::new(ByColumnName::new("UPTIME").not(Rows::first())).with(Format::content(|s| {
            let av = s.parse::<f64>().unwrap_or(-1.0);
            if av < 0.0 {
                "N/A".to_owned()
            } else {
                format!("{}%", av.round())
            }
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("PERC_US").not(Rows::first())).with(Format::content(|s| {
            let av = s.parse::<f64>().unwrap_or(-1.0);
            if av < 0.0 {
                "N/A".to_owned()
            } else {
                format!("{av:.1}%")
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
        Modify::new(ByColumnName::new("TOTAL_SATS").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("MIN_HTLC").not(Rows::first())).with(Format::content(|s| {
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
        Modify::new(ByColumnName::new("IN_BASE").not(Rows::first())).with(Format::content(|s| {
            if let Ok(b) = s.parse::<u64>() {
                u64_to_sat_string(config, b).unwrap()
            } else {
                s.to_owned()
            }
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("PPM").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("IN_PPM").not(Rows::first())).with(Format::content(|s| {
            if let Ok(b) = s.parse::<u64>() {
                u64_to_sat_string(config, b).unwrap()
            } else {
                s.to_owned()
            }
        })),
    );
    sumtable.with(
        Modify::new(ByColumnName::new("PING").not(Rows::first())).with(Format::content(|s| {
            let ping = s.parse::<u64>().unwrap();
            if ping > PING_TIMEOUT_MS || ping == 0 {
                "N/A".to_owned()
            } else {
                u64_to_sat_string(config, ping).unwrap()
            }
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
                );
            } else {
                address = Some(addr.first().unwrap().clone());
            }
        }
    }
    let mut bindaddr = None;
    if address.is_none() {
        if let Some(bind) = &getinfo.binding {
            if !bind.is_empty() {
                bindaddr = Some(bind.first().unwrap().clone());
            }
        }
    }
    match address {
        Some(a) => {
            getinfo.id.to_string()
                + "@"
                + &a.address.unwrap_or("missing address".to_owned())
                + ":"
                + &a.port.to_string()
        }
        None => match bindaddr {
            Some(baddr) => {
                getinfo.id.to_string()
                    + "@"
                    + &baddr.address.unwrap_or("missing address".to_owned())
                    + ":"
                    + &baddr.port.unwrap_or(9735).to_string()
            }
            None => "No addresses found!".to_owned(),
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
    let draw = if config.utf8 { &draw_utf8 } else { &draw_ascii };
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
