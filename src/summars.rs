use anyhow::{anyhow, Error};
use chrono::prelude::DateTime;
use chrono::{Local, Utc};
use cln_plugin::Plugin;
use cln_rpc::{
    model::*,
    primitives::{Amount, PublicKey, ShortChannelId},
};
use core::fmt;
use log::debug;
use parking_lot::Mutex;

use num_format::ToFormattedString;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    cmp::Ordering::Equal,
    collections::HashMap,
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};
use std::{path::PathBuf, str::FromStr};
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::{
    format::Format,
    locator::ByColumnName,
    object::{Object, Rows},
    Alignment, Disable, Modify, Style, TableIteratorExt, Tabled, Width,
};
use tokio::time::Instant;

use crate::config::Config;
use crate::{
    config::validateargs, get_info, list_forwards, list_funds, list_nodes, list_peers,
    make_rpc_path, PluginState,
};
use crate::{list_invoices, list_pays};

pub const NO_ALIAS_SET: &str = "NO_ALIAS_SET";
pub const NODE_GOSSIP_MISS: &str = "NODE_GOSSIP_MISS";

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ScidWrapper {
    block: u32,
    txindex: u32,
    outnum: u16,
}
impl fmt::Display for ScidWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{}x{}",
            self.block.to_string(),
            self.txindex.to_string(),
            self.outnum.to_string()
        )
    }
}
#[derive(Debug, Deserialize, Serialize, Copy, Clone)]
pub struct PeerAvailability {
    pub count: u64,
    pub connected: bool,
    pub avail: f64,
}

#[derive(Debug, Tabled)]
struct Forwards {
    #[tabled(skip)]
    received: u64,
    #[tabled(rename = "forwards")]
    received_str: String,
    in_channel: String,
    out_channel: String,
    in_sats: String,
    out_sats: String,
    fee_msats: String,
}

#[derive(Debug, Tabled)]
struct Pays {
    #[tabled(skip)]
    completed_at: u64,
    #[tabled(rename = "pays")]
    completed_at_str: String,
    payment_hash: String,
    destination: String,
}

#[derive(Debug, Tabled)]
struct Invoices {
    #[tabled(skip)]
    paid_at: u64,
    #[tabled(rename = "invoices")]
    paid_at_str: String,
    label: String,
    amount_received: String,
}

#[derive(Debug, Tabled, FieldNamesAsArray)]
#[allow(non_snake_case)]
pub struct Summary {
    OUT_SATS: String,
    IN_SATS: String,
    #[tabled(skip)]
    #[field_names_as_array(skip)]
    SCID_RAW: ScidWrapper,
    SCID: String,
    MAX_HTLC: String,
    FLAG: String,
    BASE: String,
    PPM: String,
    ALIAS: String,
    PEER_ID: String,
    UPTIME: f64,
    HTLCS: usize,
    STATE: String,
}
impl Summary {
    pub fn field_names_to_string() -> String {
        Summary::FIELD_NAMES_AS_ARRAY
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}
pub async fn summars(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let now = Instant::now();

    let rpc_path = make_rpc_path(&p);

    let mut config = p.state().config.lock().clone();
    config = validateargs(v, config)?;

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

    let funds = list_funds(&rpc_path).await?;
    debug!(
        "Listfunds. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let mut utxo_amt: u64 = 0;
    for utxo in &funds.outputs {
        match utxo.status {
            ListfundsOutputsStatus::CONFIRMED => utxo_amt += Amount::msat(&utxo.amount_msat),
            _ => (),
        }
    }

    let mut channel_count = 0;
    let mut num_connected = 0;
    let mut avail_in = 0;
    let mut avail_out = 0;
    let mut num_gossipers = 0;
    let mut table = Vec::new();
    let mut chanpeermap: HashMap<String, String> = HashMap::new();
    for peer in &peers {
        if peer.channels.len() == 0 {
            num_gossipers += 1;
        }
        for chan in &peer.channels {
            let alias = get_alias(&rpc_path, p.state().alias_map.clone(), peer.id).await?;
            match chan.short_channel_id {
                Some(i) => chanpeermap.insert(i.to_string(), peer.id.to_string()),
                None => None,
            };
            let statestr = match chan.state {
                ListpeersPeersChannelsState::OPENINGD => "OPENING",
                ListpeersPeersChannelsState::CHANNELD_AWAITING_LOCKIN => "AWAIT_LOCK",
                ListpeersPeersChannelsState::CHANNELD_NORMAL => "OK",
                ListpeersPeersChannelsState::CHANNELD_SHUTTING_DOWN => "SHUTTING_DOWN",
                ListpeersPeersChannelsState::CLOSINGD_SIGEXCHANGE => "CLOSINGD_SIGEX",
                ListpeersPeersChannelsState::CLOSINGD_COMPLETE => "CLOSINGD_DONE",
                ListpeersPeersChannelsState::AWAITING_UNILATERAL => "AWAIT_UNILATERAL",
                ListpeersPeersChannelsState::FUNDING_SPEND_SEEN => "FUNDING_SPEND",
                ListpeersPeersChannelsState::ONCHAIN => "ONCHAIN",
                ListpeersPeersChannelsState::DUALOPEND_OPEN_INIT => "DUAL_OPEN",
                ListpeersPeersChannelsState::DUALOPEND_AWAITING_LOCKIN => "DUAL_AWAIT",
            };
            let to_us_msat = Amount::msat(
                &chan
                    .to_us_msat
                    .ok_or(anyhow!("Channel with {} has no msats to us!", peer.id))?,
            );
            let total_msat = Amount::msat(
                &chan
                    .total_msat
                    .ok_or(anyhow!("Channel with {} has no total amount!", peer.id))?,
            );
            let our_reserve = Amount::msat(
                &chan
                    .our_reserve_msat
                    .ok_or(anyhow!("Channel with {} has no our_reserve!", peer.id))?,
            );
            let their_reserve = Amount::msat(
                &chan
                    .their_reserve_msat
                    .ok_or(anyhow!("Channel with {} has no their_reserve!", peer.id))?,
            );
            match chan.state {
                ListpeersPeersChannelsState::CHANNELD_NORMAL => {
                    if our_reserve < to_us_msat {
                        avail_out += to_us_msat - our_reserve
                    }
                    if their_reserve < total_msat - to_us_msat {
                        avail_in += total_msat - to_us_msat - their_reserve
                    }
                }
                _ => (),
            }

            let scidsortdummy = ShortChannelId::from_str("999999x9999x99").unwrap();
            let scid = match chan.short_channel_id {
                Some(scid) => scid,
                None => scidsortdummy,
            };
            let avail;
            match p.state().avail.lock().get(&peer.id.to_string()) {
                Some(a) => avail = a.avail,
                None => avail = -1.0,
            };
            table.push(Summary {
                OUT_SATS: (to_us_msat / 1_000).to_formatted_string(&config.locale.1),
                IN_SATS: ((total_msat - to_us_msat) / 1_000).to_formatted_string(&config.locale.1),
                SCID_RAW: ScidWrapper {
                    block: scid.block(),
                    txindex: scid.txindex(),
                    outnum: scid.outnum(),
                },
                SCID: if scidsortdummy.to_string() == scid.to_string() {
                    "PENDING".to_string()
                } else {
                    scid.to_string()
                },
                MAX_HTLC: (Amount::msat(&chan.maximum_htlc_out_msat.unwrap()) / 1_000)
                    .to_formatted_string(&config.locale.1),
                FLAG: make_channel_flags(chan.private, peer.connected),
                BASE: (Amount::msat(&chan.fee_base_msat.unwrap()))
                    .to_formatted_string(&config.locale.1),
                PPM: chan
                    .fee_proportional_millionths
                    .unwrap()
                    .to_formatted_string(&config.locale.1),
                ALIAS: if config.utf8.1 {
                    alias.to_string()
                } else {
                    alias.replace(|c: char| !c.is_ascii(), "?")
                },
                PEER_ID: peer.id.to_string(),
                UPTIME: avail * 100.0,
                HTLCS: chan.htlcs.clone().unwrap_or(Vec::new()).len(),
                STATE: statestr.to_string(),
            });

            if is_active_state(chan) {
                if peer.connected {
                    num_connected += 1
                }
                channel_count += 1;
            }
        }
    }
    debug!(
        "First peerloop. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    match config.sort_by.1.clone() {
        col if col.eq("OUT_SATS") => table.sort_by_key(|x| x.OUT_SATS.clone()),
        col if col.eq("IN_SATS") => table.sort_by_key(|x| x.IN_SATS.clone()),
        col if col.eq("SCID_RAW") => table.sort_by_key(|x| x.SCID_RAW.clone()),
        col if col.eq("SCID") => table.sort_by_key(|x| x.SCID_RAW.clone()),
        col if col.eq("MAX_HTLC") => table.sort_by_key(|x| x.MAX_HTLC.clone()),
        col if col.eq("FLAG") => table.sort_by_key(|x| x.FLAG.clone()),
        col if col.eq("BASE") => table.sort_by_key(|x| x.BASE.clone()),
        col if col.eq("PPM") => table.sort_by_key(|x| x.PPM.clone()),
        col if col.eq("ALIAS") => table.sort_by_key(|x| x.ALIAS.to_ascii_lowercase()),
        col if col.eq("UPTIME") => {
            table.sort_by(|x, y| x.UPTIME.partial_cmp(&y.UPTIME).unwrap_or(Equal))
        }
        col if col.eq("PEER_ID") => table.sort_by_key(|x| x.PEER_ID.clone()),
        col if col.eq("HTLCS") => table.sort_by_key(|x| x.HTLCS.clone()),
        col if col.eq("STATE") => table.sort_by_key(|x| x.STATE.clone()),
        _ => table.sort_by_key(|x| x.SCID_RAW.clone()),
    };
    debug!(
        "Sort Table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let mut sumtable = table.table();

    sumtable.with(Style::modern());
    if !config.show_pubkey.1 {
        sumtable.with(Disable::column(ByColumnName::new("PEER_ID")));
    }
    if !config.show_maxhtlc.1 {
        sumtable.with(Disable::column(ByColumnName::new("MAX_HTLC")));
    }
    sumtable.with(
        Modify::new(ByColumnName::new("ALIAS"))
            .with(Width::truncate(config.max_alias_length.1).suffix("[..]")),
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
        Modify::new(ByColumnName::new("UPTIME").not(Rows::first())).with(Format::new(|s| {
            let av = s.parse::<f64>().unwrap_or(-1.0);
            if av < 0.0 {
                "N/A".to_string()
            } else {
                format!("{}%", av.round())
            }
        })),
    );

    sumtable.with(Modify::new(Rows::first()).with(Alignment::center()));
    debug!(
        "End of sum table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );

    let forwards;
    if config.forwards.1 > 0 {
        forwards = Some(
            recent_forwards(&rpc_path, &chanpeermap, &p.state().alias_map, &config, now).await?,
        );
        debug!(
            "End of fw table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        forwards = None;
    }

    let pays;
    if config.pays.1 > 0 {
        pays = Some(
            recent_pays(
                &rpc_path,
                p.state().alias_map.clone(),
                &config,
                now,
                getinfo.id,
            )
            .await?,
        );
        debug!(
            "End of pays table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        pays = None;
    }

    let invoices;
    if config.invoices.1 > 0 {
        invoices = Some(recent_invoices(&rpc_path, &config, now).await?);
        debug!(
            "End of invoices table. Total: {}ms",
            now.elapsed().as_millis().to_string()
        );
    } else {
        invoices = None;
    }

    let mut address = None;
    if let Some(addr) = getinfo.address {
        if addr.len() > 0 {
            if addr.iter().any(|x| match x.item_type {
                GetinfoAddressType::IPV4 => true,
                _ => false,
            }) {
                address = Some(
                    addr.iter()
                        .find(|x| match x.item_type {
                            GetinfoAddressType::IPV4 => true,
                            _ => false,
                        })
                        .unwrap()
                        .clone(),
                )
            } else {
                address = Some(addr.first().unwrap().clone())
            }
        }
    }
    let mut bindaddr = None;
    if let None = address {
        if let Some(bind) = getinfo.binding {
            if bind.len() > 0 {
                bindaddr = Some(bind.first().unwrap().clone())
            }
        }
    }
    let addr_str = match address {
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
    };

    let mut result = sumtable.to_string();
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
utxo_amount={:.8} BTC
num_channels={}
num_connected={}
num_gossipers={}
avail_out={:.8} BTC
avail_in={:.8} BTC
fees_collected={:.8} BTC
channels_flags=P:private O:offline
{}",
        addr_str,
        funds.outputs.len(),
        utxo_amt as f64 / 100_000_000_000.0,
        channel_count,
        num_connected,
        num_gossipers,
        avail_out as f64 / 100_000_000_000.0,
        avail_in as f64 / 100_000_000_000.0,
        Amount::msat(&getinfo.fees_collected_msat) as f64 / 100_000_000_000.0,
        result,
    )}))
}

async fn recent_forwards(
    rpc_path: &PathBuf,
    chanpeermap: &HashMap<String, String>,
    alias_map: &Arc<Mutex<HashMap<String, String>>>,
    config: &Config,
    now: Instant,
) -> Result<String, Error> {
    let forwards = list_forwards(rpc_path, Some(ListforwardsStatus::SETTLED), None, None)
        .await?
        .forwards;
    debug!(
        "List forwards. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    let mut table = Vec::new();
    let alias_map = alias_map.lock().clone();
    for forward in forwards {
        if forward.received_time as u64
            > Utc::now().timestamp() as u64 - config.forwards.1 * 60 * 60
        {
            let d = UNIX_EPOCH + Duration::from_millis((forward.received_time * 1000.0) as u64);
            let datetime = DateTime::<Local>::from(d);
            let timestamp_str = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
            let inchan = if config.forward_alias.1 {
                match chanpeermap.get(&forward.in_channel.to_string()) {
                    Some(peer) => match alias_map.get::<String>(peer) {
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
            let fw_outchan = forward.out_channel.unwrap().to_string();
            let outchan = if config.forward_alias.1 {
                match chanpeermap.get(&fw_outchan) {
                    Some(peer) => match alias_map.get::<String>(peer) {
                        Some(alias) => {
                            if alias.eq(NO_ALIAS_SET) {
                                fw_outchan
                            } else {
                                alias.clone()
                            }
                        }
                        None => fw_outchan,
                    },
                    None => fw_outchan,
                }
            } else {
                fw_outchan
            };
            table.push(Forwards {
                received: (forward.received_time * 1000.0) as u64,
                received_str: timestamp_str,
                in_channel: if config.utf8.1 {
                    inchan
                } else {
                    inchan.replace(|c: char| !c.is_ascii(), "?")
                },
                out_channel: if config.utf8.1 {
                    outchan
                } else {
                    outchan.replace(|c: char| !c.is_ascii(), "?")
                },
                in_sats: (Amount::msat(&forward.in_msat) / 1000)
                    .to_formatted_string(&config.locale.1),
                out_sats: (Amount::msat(&forward.out_msat.unwrap()) / 1000)
                    .to_formatted_string(&config.locale.1),
                fee_msats: Amount::msat(&forward.fee_msat.unwrap())
                    .to_formatted_string(&config.locale.1),
            })
        }
    }
    debug!(
        "Build forwards table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    table.sort_by_key(|x| x.received.clone());
    let mut fwtable = table.table();
    fwtable.with(Style::blank());
    fwtable.with(
        Modify::new(ByColumnName::new("in_channel"))
            .with(Width::truncate(config.max_alias_length.1).suffix("[..]")),
    );
    fwtable.with(
        Modify::new(ByColumnName::new("out_channel"))
            .with(Width::truncate(config.max_alias_length.1).suffix("[..]")),
    );
    fwtable.with(Modify::new(ByColumnName::new("in_sats")).with(Alignment::right()));
    fwtable.with(Modify::new(ByColumnName::new("out_sats")).with(Alignment::right()));
    fwtable.with(Modify::new(ByColumnName::new("fee_msats")).with(Alignment::right()));
    Ok(fwtable.to_string())
}

async fn recent_pays(
    rpc_path: &PathBuf,
    alias_map: Arc<Mutex<HashMap<String, String>>>,
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
        if pay.completed_at.unwrap() > Utc::now().timestamp() as u64 - config.pays.1 * 60 * 60
            && pay.destination.unwrap() != mypubkey
        {
            let d = UNIX_EPOCH + Duration::from_secs(pay.completed_at.unwrap());
            let datetime = DateTime::<Local>::from(d);
            let timestamp_str = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
            let destination =
                get_alias(rpc_path, alias_map.clone(), pay.destination.unwrap()).await?;
            table.push(Pays {
                completed_at: pay.completed_at.unwrap(),
                completed_at_str: timestamp_str,
                payment_hash: pay.payment_hash.to_string(),
                destination,
            })
        }
    }
    debug!(
        "Build pays table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    table.sort_by_key(|x| x.completed_at);
    let mut paystable = table.table();
    paystable.with(Style::blank());
    Ok(paystable.to_string())
}

async fn recent_invoices(
    rpc_path: &PathBuf,
    config: &Config,
    now: Instant,
) -> Result<String, Error> {
    let invoices = list_invoices(rpc_path, None, None).await?.invoices;
    debug!(
        "List invoices. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    let mut table = Vec::new();
    for invoice in invoices {
        match invoice.status {
            ListinvoicesInvoicesStatus::PAID => {
                if invoice.paid_at.unwrap()
                    > Utc::now().timestamp() as u64 - config.invoices.1 * 60 * 60
                {
                    let d = UNIX_EPOCH + Duration::from_secs(invoice.paid_at.unwrap());
                    let datetime = DateTime::<Local>::from(d);
                    let timestamp_str = datetime.format("%Y-%m-%d %H:%M:%S").to_string();

                    table.push(Invoices {
                        paid_at: invoice.paid_at.unwrap(),
                        paid_at_str: timestamp_str,
                        label: invoice.label,
                        amount_received: Amount::msat(&invoice.amount_received_msat.unwrap())
                            .to_formatted_string(&config.locale.1),
                    })
                }
            }
            _ => (),
        }
    }
    debug!(
        "Build invoices table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    table.sort_by_key(|x| x.paid_at);
    let mut invoicestable = table.table();
    invoicestable.with(Style::blank());
    invoicestable.with(Modify::new(ByColumnName::new("amount_received")).with(Alignment::right()));
    Ok(invoicestable.to_string())
}

async fn get_alias(
    rpc_path: &PathBuf,
    alias_map: Arc<Mutex<HashMap<String, String>>>,
    peer_id: PublicKey,
) -> Result<String, Error> {
    let alias_map_clone = alias_map.lock().clone();
    let alias;
    match alias_map_clone.get::<String>(&peer_id.to_string()) {
        Some(a) => alias = a.clone(),
        None => match list_nodes(&rpc_path, &peer_id).await?.nodes.first() {
            Some(node) => {
                match &node.alias {
                    Some(newalias) => alias = newalias.clone(),
                    None => alias = NO_ALIAS_SET.to_string(),
                }
                alias_map.lock().insert(peer_id.to_string(), alias.clone());
            }
            None => alias = NODE_GOSSIP_MISS.to_string(),
        },
    };
    Ok(alias)
}

fn make_channel_flags(private: Option<bool>, connected: bool) -> String {
    let mut flags = String::from("[");
    match private {
        Some(is_priv) => {
            if is_priv {
                flags.push_str("P")
            } else {
                flags.push_str("_")
            }
        }
        None => flags.push_str("E"),
    }
    if connected {
        flags.push_str("_")
    } else {
        flags.push_str("O")
    }
    flags.push_str("]");
    flags
}
#[test]
fn test_flags() {
    assert_eq!(make_channel_flags(Some(false), true), "[__]");
    assert_eq!(make_channel_flags(Some(true), true), "[P_]");
    assert_eq!(make_channel_flags(Some(false), false), "[_O]");
    assert_eq!(make_channel_flags(Some(true), false), "[PO]");
    assert_eq!(make_channel_flags(None, true), "[E_]");
    assert_eq!(make_channel_flags(None, false), "[EO]");
}

pub fn is_active_state(channel: &ListpeersPeersChannels) -> bool {
    match channel.state {
        ListpeersPeersChannelsState::OPENINGD => true,
        ListpeersPeersChannelsState::CHANNELD_AWAITING_LOCKIN => true,
        ListpeersPeersChannelsState::CHANNELD_NORMAL => true,
        ListpeersPeersChannelsState::DUALOPEND_OPEN_INIT => true,
        ListpeersPeersChannelsState::DUALOPEND_AWAITING_LOCKIN => true,
        _ => false,
    }
}
