use std::{cmp::Reverse, collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use anyhow::{anyhow, Error};
use cln_plugin::Plugin;
use cln_rpc::{
    model::{
        requests::{
            GetinfoRequest,
            ListfundsRequest,
            ListpeerchannelsRequest,
            ListpeersRequest,
            PingRequest,
        },
        responses::{
            GetinfoAddressType,
            GetinfoResponse,
            ListfundsOutputsStatus,
            ListfundsResponse,
            ListpeerchannelsChannels,
            ListpeersPeers,
        },
    },
    primitives::{Amount, ChannelState, PublicKey, ShortChannelId},
    ClnRpc,
};
use serde_json::json;
use strum::IntoEnumIterator;
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
    forwards::{format_forwards, gather_forwards_data},
    invoices::{format_invoices, gather_invoices_data},
    pays::{format_pays, gather_pays_data},
    structs::{
        ChannelVisibility,
        Config,
        ConnectionStatus,
        FullNodeData,
        GraphCharset,
        NodeSummary,
        PluginState,
        ShortChannelState,
        Summary,
        SummaryColumns,
        TableColumn,
        NODE_GOSSIP_MISS,
    },
    util::{
        at_or_above_version,
        draw_chans_graph,
        is_active_state,
        make_channel_flags,
        make_rpc_path,
        perc_trunc_u64,
        rounded_div_u64,
        sort_columns,
        u64_to_btc_string,
        u64_to_sat_string,
    },
};

const PING_TIMEOUT_MS: u64 = 5000;

pub async fn summary(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let now = Instant::now();

    let rpc_path = make_rpc_path(&plugin);
    let mut rpc = ClnRpc::new(&rpc_path).await?;

    let mut config = plugin.state().config.lock().clone();
    validateargs(args, &mut config)?;
    log::debug!("Args validated. Total: {}ms", now.elapsed().as_millis());

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

    let mut full_node_data = FullNodeData::new(
        getinfo.id,
        getinfo.version.clone(),
        graph_max_chan_side_msat,
    );

    build_node_data(
        now,
        &peer_channels,
        &peers,
        &config,
        &mut rpc,
        plugin.clone(),
        &mut full_node_data,
    )
    .await?;

    node_data_to_output(now, &config, &mut full_node_data, &getinfo, &funds)
}

async fn build_node_data(
    now: Instant,
    peer_channels: &[ListpeerchannelsChannels],
    peers: &[ListpeersPeers],
    config: &Config,
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    process_channels_data(
        now,
        peer_channels,
        peers,
        config,
        plugin.clone(),
        full_node_data,
    )
    .await?;
    log::debug!(
        "End of channels table. Total: {}ms",
        now.elapsed().as_millis()
    );

    if config.forwards > 0 {
        gather_forwards_data(
            rpc,
            peer_channels,
            plugin.clone(),
            config,
            now,
            full_node_data,
        )
        .await?;
        log::debug!(
            "End of forwards table. Total: {}ms",
            now.elapsed().as_millis()
        );
    }

    if config.pays > 0 {
        gather_pays_data(
            rpc,
            plugin.clone(),
            config,
            peer_channels,
            now,
            full_node_data,
        )
        .await?;
        log::debug!("End of pays table. Total: {}ms", now.elapsed().as_millis());
    }

    if config.invoices > 0 {
        gather_invoices_data(plugin.clone(), rpc, config, now, full_node_data).await?;
        log::debug!(
            "End of invoices table. Total: {}ms",
            now.elapsed().as_millis()
        );
    }

    Ok(())
}

fn node_data_to_output(
    now: Instant,
    config: &Config,
    node_data: &mut FullNodeData,
    getinfo: &GetinfoResponse,
    funds: &ListfundsResponse,
) -> Result<serde_json::Value, Error> {
    let addr_str = get_addrstr(getinfo);

    let mut utxo_amt: u64 = 0;
    for utxo in &funds.outputs {
        if let ListfundsOutputsStatus::CONFIRMED = utxo.status {
            utxo_amt += Amount::msat(&utxo.amount_msat);
        }
    }

    let utxo_amount = format!("{} BTC", u64_to_btc_string(config, utxo_amt)?);
    let avail_out = format!(
        "{} BTC",
        u64_to_btc_string(config, node_data.node_summary.avail_out)?
    );
    let avail_in = format!(
        "{} BTC",
        u64_to_btc_string(config, node_data.node_summary.avail_in)?
    );
    let fees_collected = format!(
        "{} BTC",
        u64_to_btc_string(config, Amount::msat(&getinfo.fees_collected_msat))?
    );

    if config.json {
        Ok(json!({"info":{
            "address":addr_str,
            "num_utxos":funds.outputs.len(),
            "utxo_amount": utxo_amount,
            "num_channels":node_data.node_summary.channel_count,
            "num_connected":node_data.node_summary.num_connected,
            "num_gossipers":node_data.node_summary.num_gossipers,
            "avail_out":avail_out,
            "avail_in":avail_in,
            "fees_collected":fees_collected,
        },
        "channels":node_data.channels,
        "forwards":node_data.forwards,
        "pays":node_data.pays,
        "invoices":node_data.invoices,
        "totals":node_data.totals}))
    } else {
        let mut sumtable = Table::new(&node_data.channels);
        format_summary(config, &mut sumtable)?;
        draw_graph_sats_name(config, &mut sumtable, node_data.graph_max_chan_side_msat)?;
        log::debug!("Format summary. Total: {}ms", now.elapsed().as_millis());

        if node_data.node_summary.filter_count > 0 {
            sumtable.with(Panel::footer(format!(
                "\n {} channel{} filtered.",
                node_data.node_summary.filter_count,
                if node_data.node_summary.filter_count == 1 {
                    ""
                } else {
                    "s"
                }
            )));
            sumtable.with(Modify::new(Rows::last()).with(Alignment::left()));
        }

        let mut result = sumtable.to_string();
        if config.forwards > 0 {
            result += &("\n\n".to_owned() + &format_forwards(config, node_data)?);
            log::debug!("Format forwards. Total: {}ms", now.elapsed().as_millis());
        }
        if config.pays > 0 {
            result += &("\n\n".to_owned() + &format_pays(config, node_data)?);
            log::debug!("Format pays. Total: {}ms", now.elapsed().as_millis());
        }
        if config.invoices > 0 {
            result += &("\n\n".to_owned() + &format_invoices(config, node_data)?);
            log::debug!("Format invoices. Total: {}ms", now.elapsed().as_millis());
        }

        Ok(json!({"format-hint":"simple","result":format!(
            "address={}
num_utxos={}
utxo_amount={}
num_channels={}
num_connected={}
num_gossipers={}
avail_out={}
avail_in={}
fees_collected={}
channels_flags=P:private O:offline
{}",
            addr_str,
            funds.outputs.len(),
            utxo_amount,
            node_data.node_summary.channel_count,
            node_data.node_summary.num_connected,
            node_data.node_summary.num_gossipers,
            avail_out,
            avail_in,
            fees_collected,
            result,
        )}))
    }
}

async fn process_channels_data(
    now: Instant,
    peer_channels: &[ListpeerchannelsChannels],
    peers: &[ListpeersPeers],
    config: &Config,
    plugin: Plugin<PluginState>,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    full_node_data.node_summary = NodeSummary {
        num_gossipers: peers
            .iter()
            .filter(|s| s.num_channels.unwrap() == 0)
            .count(),
        ..Default::default()
    };

    let mut channel_map = HashMap::with_capacity(peer_channels.len());
    {
        let alias_map = plugin.state().alias_map.lock();

        for (id, chan) in peer_channels.iter().enumerate() {
            if is_chan_filtered(config, chan) {
                full_node_data.node_summary.filter_count += 1;
                continue;
            }
            let alias = alias_map
                .get(&chan.peer_id)
                .cloned()
                .unwrap_or(NODE_GOSSIP_MISS.to_owned());

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
                    full_node_data.node_summary.avail_out += to_us_msat - our_reserve;
                }
                if their_reserve < total_msat - to_us_msat {
                    full_node_data.node_summary.avail_in += total_msat - to_us_msat - their_reserve;
                }
            }

            let avail = match plugin.state().avail.lock().get(&chan.peer_id) {
                Some(a) => a.avail,
                None => -1.0,
            };

            let summary = chan_to_summary(
                config,
                chan,
                alias,
                avail,
                full_node_data.graph_max_chan_side_msat,
            )?;
            channel_map.insert(id, summary);

            if is_active_state(chan) {
                if chan.peer_connected {
                    full_node_data.node_summary.num_connected += 1;
                }
                full_node_data.node_summary.channel_count += 1;
            }
        }
    }
    log::debug!("First summary-loop. Total: {}ms", now.elapsed().as_millis());

    get_pings(
        plugin.clone(),
        config,
        &full_node_data.cln_version,
        &mut channel_map,
    )
    .await?;
    log::debug!("Got pings. Total: {}ms", now.elapsed().as_millis());

    let mut channel_vec = channel_map.into_values().collect::<Vec<Summary>>();
    sort_summary(config, &mut channel_vec);

    full_node_data.channels = channel_vec;

    Ok(())
}

fn is_chan_filtered(config: &Config, chan: &ListpeerchannelsChannels) -> bool {
    let is_excluded_state = config
        .exclude_channel_states
        .channel_states
        .contains(&ShortChannelState(chan.state));

    let is_chan_vis_filtered =
        if let Some(excl_vis) = &config.exclude_channel_states.channel_visibility {
            match excl_vis {
                ChannelVisibility::Private => chan.private.unwrap(),
                ChannelVisibility::Public => !chan.private.unwrap(),
            }
        } else {
            false
        };

    let is_chan_con_status_filtered =
        if let Some(excl_conn) = &config.exclude_channel_states.connection_status {
            match excl_conn {
                ConnectionStatus::Online => chan.peer_connected,
                ConnectionStatus::Offline => !chan.peer_connected,
            }
        } else {
            false
        };

    is_excluded_state || is_chan_vis_filtered || is_chan_con_status_filtered
}

fn chan_to_summary(
    config: &Config,
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
    if config.columns.contains(&SummaryColumns::IN_BASE)
        || config.columns.contains(&SummaryColumns::IN_PPM)
        || config.sort_by == SummaryColumns::IN_BASE
        || config.sort_by == SummaryColumns::IN_PPM
    {
        if let Some(upd) = &chan.updates {
            if let Some(rem) = &upd.remote {
                in_base = rem.fee_base_msat.msat().to_string();
                in_ppm = rem.fee_proportional_millionths.to_string();
            }
        }
    }

    let graph_sats = if config.columns.contains(&SummaryColumns::GRAPH_SATS) {
        draw_chans_graph(config, total_msat, to_us_msat, graph_max_chan_side_msat)
    } else {
        String::new()
    };

    Ok(Summary {
        graph_sats,
        out_sats: rounded_div_u64(to_us_msat, 1_000),
        in_sats: rounded_div_u64(total_msat - to_us_msat, 1_000),
        total_sats: rounded_div_u64(total_msat, 1_000),
        scid_raw: scid,
        scid: if scidsortdummy == scid {
            "PENDING".to_owned()
        } else {
            scid.to_string()
        },
        min_htlc: rounded_div_u64(chan.minimum_htlc_out_msat.unwrap().msat(), 1_000),
        max_htlc: rounded_div_u64(chan.maximum_htlc_out_msat.unwrap().msat(), 1_000),
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
        perc_us: perc_trunc_u64(to_us_msat, total_msat),
        ping: 0,
    })
}

async fn get_pings(
    plugin: Plugin<PluginState>,
    config: &Config,
    version: &str,
    table: &mut HashMap<usize, Summary>,
) -> Result<(), Error> {
    let rpc_path = make_rpc_path(&plugin);

    if !at_or_above_version(version, "25.09")? {
        log::info!("Not using ping on pre-v25.09 CLN");
        return Ok(());
    } else if config.columns.contains(&SummaryColumns::PING)
        || config.sort_by == SummaryColumns::PING
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
                        let elap = u64::try_from(now.elapsed().as_millis()).unwrap();
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
    macro_rules! sort_by_key {
        ($key:expr) => {
            if config.sort_reverse {
                table.sort_by_key(|x| Reverse($key(x)));
            } else {
                table.sort_by_key($key);
            }
        };
    }

    match config.sort_by {
        SummaryColumns::OUT_SATS => sort_by_key!(|x: &Summary| x.out_sats),
        SummaryColumns::IN_SATS => sort_by_key!(|x: &Summary| x.in_sats),
        SummaryColumns::TOTAL_SATS => sort_by_key!(|x: &Summary| x.total_sats),
        SummaryColumns::MIN_HTLC => sort_by_key!(|x: &Summary| x.min_htlc),
        SummaryColumns::MAX_HTLC => sort_by_key!(|x: &Summary| x.max_htlc),
        SummaryColumns::FLAG => sort_by_key!(|x: &Summary| x.flag.clone()),
        SummaryColumns::BASE => sort_by_key!(|x: &Summary| x.base),
        SummaryColumns::PPM => sort_by_key!(|x: &Summary| x.ppm),
        SummaryColumns::PEER_ID => sort_by_key!(|x: &Summary| x.peer_id),
        SummaryColumns::HTLCS => sort_by_key!(|x: &Summary| x.htlcs),
        SummaryColumns::STATE => sort_by_key!(|x: &Summary| x.state.clone()),
        SummaryColumns::PING => sort_by_key!(|x: &Summary| x.ping),
        SummaryColumns::SCID | SummaryColumns::GRAPH_SATS => sort_by_key!(|x: &Summary| x.scid_raw),
        SummaryColumns::IN_BASE => {
            sort_by_key!(|x: &Summary| x.in_base.parse().unwrap_or(u64::MAX));
        }
        SummaryColumns::IN_PPM => sort_by_key!(|x: &Summary| x.in_ppm.parse().unwrap_or(u64::MAX)),
        SummaryColumns::ALIAS => sort_by_key!(|x: &Summary| {
            x.alias
                .chars()
                .filter(|c| c.is_ascii() && !c.is_whitespace() && *c != '@')
                .collect::<String>()
                .to_ascii_lowercase()
        }),
        SummaryColumns::UPTIME => table.sort_by(|a, b| {
            let ord = a
                .uptime
                .partial_cmp(&b.uptime)
                .unwrap_or(std::cmp::Ordering::Equal);
            if config.sort_reverse {
                ord.reverse()
            } else {
                ord
            }
        }),
        SummaryColumns::PERC_US => table.sort_by(|a, b| {
            let ord = a
                .perc_us
                .partial_cmp(&b.perc_us)
                .unwrap_or(std::cmp::Ordering::Equal);
            if config.sort_reverse {
                ord.reverse()
            } else {
                ord
            }
        }),
    }
}

#[allow(clippy::too_many_lines)]
fn format_summary(config: &Config, sumtable: &mut Table) -> Result<(), Error> {
    config.style.apply(sumtable);
    for head in SummaryColumns::iter() {
        if !config.columns.contains(&head) {
            sumtable.with(Remove::column(ByColumnName::new(head.to_string())));
        }
    }

    let headers = sumtable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| SummaryColumns::parse_column(s.text()).unwrap())
        .collect::<Vec<SummaryColumns>>();
    let records = sumtable.get_records_mut();
    if headers.len() != config.columns.len() {
        return Err(anyhow!(
            "Error formatting channels! Length difference detected: {} {}",
            SummaryColumns::to_list_string(&headers),
            SummaryColumns::to_list_string(&config.columns)
        ));
    }
    sort_columns(records, &headers, &config.columns);

    for numerical in SummaryColumns::NUMERICAL {
        sumtable
            .with(Modify::new(ByColumnName::new(numerical.to_string())).with(Alignment::right()));
        sumtable.with(
            Modify::new(ByColumnName::new(numerical.to_string()).not(Rows::first())).with(
                Format::content(|s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()),
            ),
        );
    }

    for opt_num in SummaryColumns::OPTIONAL_NUMERICAL {
        sumtable.with(Modify::new(ByColumnName::new(opt_num.to_string())).with(Alignment::right()));
        sumtable.with(
            Modify::new(ByColumnName::new(opt_num.to_string()).not(Rows::first())).with(
                Format::content(|s| {
                    if let Ok(b) = s.parse::<u64>() {
                        u64_to_sat_string(config, b).unwrap()
                    } else {
                        s.to_owned()
                    }
                }),
            ),
        );
    }

    if config.max_alias_length < 0 {
        sumtable.with(
            Modify::new(ByColumnName::new(SummaryColumns::ALIAS.to_string())).with(
                Width::wrap(usize::try_from(config.max_alias_length.unsigned_abs())?)
                    .keep_words(true),
            ),
        );
    } else {
        sumtable.with(
            Modify::new(ByColumnName::new(SummaryColumns::ALIAS.to_string()))
                .with(Width::truncate(usize::try_from(config.max_alias_length)?).suffix("[..]")),
        );
    }

    sumtable.with(
        Modify::new(ByColumnName::new(SummaryColumns::FLAG.to_string())).with(Alignment::center()),
    );
    sumtable.with(
        Modify::new(ByColumnName::new(SummaryColumns::HTLCS.to_string())).with(Alignment::right()),
    );
    sumtable.with(
        Modify::new(ByColumnName::new(SummaryColumns::STATE.to_string())).with(Alignment::center()),
    );
    sumtable.with(
        Modify::new(ByColumnName::new(SummaryColumns::PING.to_string())).with(Alignment::right()),
    );

    for percent_col in [SummaryColumns::UPTIME, SummaryColumns::PERC_US] {
        sumtable
            .with(Modify::new(ByColumnName::new(percent_col.to_string())).with(Alignment::right()));
        sumtable.with(
            Modify::new(ByColumnName::new(percent_col.to_string()).not(Rows::first())).with(
                Format::content(|s| {
                    let av = s.parse::<f64>().unwrap_or(-1.0);
                    if av < 0.0 {
                        "N/A".to_owned()
                    } else if percent_col == SummaryColumns::UPTIME {
                        format!("{}%", av.round())
                    } else {
                        format!("{av:.1}%")
                    }
                }),
            ),
        );
    }

    sumtable.with(
        Modify::new(ByColumnName::new(SummaryColumns::PING.to_string()).not(Rows::first())).with(
            Format::content(|s| {
                let ping = s.parse::<u64>().unwrap();
                if ping > PING_TIMEOUT_MS || ping == 0 {
                    "N/A".to_owned()
                } else {
                    u64_to_sat_string(config, ping).unwrap()
                }
            }),
        ),
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
