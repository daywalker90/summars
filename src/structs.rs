use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    str::FromStr,
    sync::Arc,
};

use anyhow::anyhow;
use cln_plugin::Error;
use cln_rpc::{
    model::responses::ListpeerchannelsChannelsState,
    primitives::{PublicKey, ShortChannelId},
};
use icu_locid::Locale;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use struct_field_names_as_array::FieldNamesAsArray;
use sys_locale::get_locale;
use tabled::{settings::Style, Table, Tabled};

use crate::{
    OPT_AVAILABILITY_INTERVAL, OPT_AVAILABILITY_WINDOW, OPT_COLUMNS, OPT_EXCLUDE_CHANNEL_STATES,
    OPT_FLOW_STYLE, OPT_FORWARDS, OPT_FORWARDS_ALIAS, OPT_FORWARDS_FILTER_AMT,
    OPT_FORWARDS_FILTER_FEE, OPT_INVOICES, OPT_INVOICES_FILTER_AMT, OPT_LOCALE,
    OPT_MAX_ALIAS_LENGTH, OPT_PAYS, OPT_REFRESH_ALIAS, OPT_SORT_BY, OPT_STYLE, OPT_UTF8,
};

pub const NO_ALIAS_SET: &str = "NO_ALIAS_SET";
pub const NODE_GOSSIP_MISS: &str = "NODE_GOSSIP_MISS";

#[derive(Clone, Debug)]
pub struct Config {
    pub columns: DynamicConfigOption<Vec<String>>,
    pub sort_by: DynamicConfigOption<String>,
    pub exclude_channel_states: DynamicConfigOption<Vec<ShortChannelState>>,
    pub forwards: DynamicConfigOption<u64>,
    pub forwards_filter_amt_msat: DynamicConfigOption<i64>,
    pub forwards_filter_fee_msat: DynamicConfigOption<i64>,
    pub forwards_alias: DynamicConfigOption<bool>,
    pub pays: DynamicConfigOption<u64>,
    pub invoices: DynamicConfigOption<u64>,
    pub invoices_filter_amt_msat: DynamicConfigOption<i64>,
    pub locale: DynamicConfigOption<Locale>,
    pub refresh_alias: DynamicConfigOption<u64>,
    pub max_alias_length: DynamicConfigOption<u64>,
    pub availability_interval: DynamicConfigOption<u64>,
    pub availability_window: DynamicConfigOption<u64>,
    pub utf8: DynamicConfigOption<bool>,
    pub style: DynamicConfigOption<Styles>,
    pub flow_style: DynamicConfigOption<Styles>,
}
impl Config {
    pub fn new() -> Config {
        Config {
            columns: DynamicConfigOption {
                name: OPT_COLUMNS.name,
                value: {
                    Summary::FIELD_NAMES_AS_ARRAY
                        .into_iter()
                        .filter(|t| t != &"GRAPH_SATS")
                        .map(|s| s.to_string())
                        .collect::<Vec<String>>()
                },
            },
            sort_by: DynamicConfigOption {
                name: OPT_SORT_BY.name,
                value: "SCID".to_string(),
            },
            exclude_channel_states: DynamicConfigOption {
                name: OPT_EXCLUDE_CHANNEL_STATES.name,
                value: Vec::new(),
            },
            forwards: DynamicConfigOption {
                name: OPT_FORWARDS.name,
                value: 0,
            },
            forwards_filter_amt_msat: DynamicConfigOption {
                name: OPT_FORWARDS_FILTER_AMT.name,
                value: -1,
            },
            forwards_filter_fee_msat: DynamicConfigOption {
                name: OPT_FORWARDS_FILTER_FEE.name,
                value: -1,
            },
            forwards_alias: DynamicConfigOption {
                name: OPT_FORWARDS_ALIAS.name,
                value: true,
            },
            pays: DynamicConfigOption {
                name: OPT_PAYS.name,
                value: 0,
            },
            invoices: DynamicConfigOption {
                name: OPT_INVOICES.name,
                value: 0,
            },
            invoices_filter_amt_msat: DynamicConfigOption {
                name: OPT_INVOICES_FILTER_AMT.name,
                value: -1,
            },
            locale: DynamicConfigOption {
                name: OPT_LOCALE.name,
                value: match get_locale() {
                    Some(l) => {
                        if l.eq(&'C'.to_string()) {
                            Locale::from_str("en-US").unwrap()
                        } else {
                            match Locale::from_str(&l) {
                                Ok(sl) => sl,
                                Err(_) => Locale::from_str("en-US").unwrap(),
                            }
                        }
                    }
                    None => Locale::from_str("en-US").unwrap(),
                },
            },
            refresh_alias: DynamicConfigOption {
                name: OPT_REFRESH_ALIAS.name,
                value: 24,
            },
            max_alias_length: DynamicConfigOption {
                name: OPT_MAX_ALIAS_LENGTH.name,
                value: 20,
            },
            availability_interval: DynamicConfigOption {
                name: OPT_AVAILABILITY_INTERVAL.name,
                value: 300,
            },
            availability_window: DynamicConfigOption {
                name: OPT_AVAILABILITY_WINDOW.name,
                value: 72,
            },
            utf8: DynamicConfigOption {
                name: OPT_UTF8.name,
                value: true,
            },
            style: DynamicConfigOption {
                name: OPT_STYLE.name,
                value: Styles::Psql,
            },
            flow_style: DynamicConfigOption {
                name: OPT_FLOW_STYLE.name,
                value: Styles::Blank,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct DynamicConfigOption<T> {
    pub name: &'static str,
    pub value: T,
}

#[derive(Clone)]
pub struct PluginState {
    pub alias_map: Arc<Mutex<BTreeMap<PublicKey, String>>>,
    pub config: Arc<Mutex<Config>>,
    pub avail: Arc<Mutex<BTreeMap<PublicKey, PeerAvailability>>>,
    pub fw_index: Arc<Mutex<PagingIndex>>,
    pub inv_index: Arc<Mutex<PagingIndex>>,
}
impl PluginState {
    pub fn new() -> PluginState {
        PluginState {
            alias_map: Arc::new(Mutex::new(BTreeMap::new())),
            config: Arc::new(Mutex::new(Config::new())),
            avail: Arc::new(Mutex::new(BTreeMap::new())),
            fw_index: Arc::new(Mutex::new(PagingIndex::new())),
            inv_index: Arc::new(Mutex::new(PagingIndex::new())),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Copy, Clone)]
pub struct PeerAvailability {
    pub count: u64,
    pub connected: bool,
    pub avail: f64,
}

#[derive(Debug, Tabled, FieldNamesAsArray)]
#[field_names_as_array(rename_all = "SCREAMING_SNAKE_CASE")]
#[tabled(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Summary {
    pub graph_sats: String,
    pub out_sats: u64,
    pub in_sats: u64,
    #[tabled(skip)]
    #[field_names_as_array(skip)]
    pub scid_raw: ShortChannelId,
    pub scid: String,
    pub max_htlc: u64,
    pub flag: String,
    pub base: u64,
    pub ppm: u32,
    pub alias: String,
    pub peer_id: PublicKey,
    pub uptime: f64,
    pub htlcs: usize,
    pub state: String,
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

#[derive(Debug, Tabled)]
pub struct Forwards {
    #[tabled(skip)]
    pub received: u64,
    #[tabled(rename = "forwards")]
    pub received_str: String,
    pub in_channel: String,
    pub out_channel: String,
    pub in_sats: String,
    pub out_sats: String,
    pub fee_msats: String,
}

#[derive(Debug, Clone)]
pub struct PagingIndex {
    pub timestamp: u64,
    pub start: u64,
}
impl PagingIndex {
    pub fn new() -> PagingIndex {
        PagingIndex {
            timestamp: 0,
            start: 0,
        }
    }
}

#[derive(Debug, Tabled)]
pub struct Pays {
    #[tabled(skip)]
    pub completed_at: u64,
    #[tabled(rename = "pays")]
    pub completed_at_str: String,
    pub payment_hash: String,
    pub sats_sent: String,
    pub destination: String,
}

#[derive(Debug, Tabled)]
pub struct Invoices {
    #[tabled(skip)]
    pub paid_at: u64,
    #[tabled(rename = "invoices")]
    pub paid_at_str: String,
    pub label: String,
    pub sats_received: String,
}

#[derive(Debug, Clone)]
pub enum Styles {
    Ascii,
    Modern,
    Sharp,
    Rounded,
    Extended,
    Psql,
    Markdown,
    ReStructuredText,
    Dots,
    AsciiRounded,
    Blank,
    Empty,
}
impl Styles {
    pub fn apply<'a>(&'a self, table: &'a mut Table) -> &mut Table {
        match self {
            Styles::Ascii => table.with(Style::ascii()),
            Styles::Modern => table.with(Style::modern()),
            Styles::Sharp => table.with(Style::sharp()),
            Styles::Rounded => table.with(Style::rounded()),
            Styles::Extended => table.with(Style::extended()),
            Styles::Psql => table.with(Style::psql()),
            Styles::Markdown => table.with(Style::markdown()),
            Styles::ReStructuredText => table.with(Style::re_structured_text()),
            Styles::Dots => table.with(Style::dots()),
            Styles::AsciiRounded => table.with(Style::ascii_rounded()),
            Styles::Blank => table.with(Style::blank()),
            Styles::Empty => table.with(Style::empty()),
        }
    }
}
impl FromStr for Styles {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ascii" => Ok(Styles::Ascii),
            "modern" => Ok(Styles::Modern),
            "sharp" => Ok(Styles::Sharp),
            "rounded" => Ok(Styles::Rounded),
            "extended" => Ok(Styles::Extended),
            "psql" => Ok(Styles::Psql),
            "markdown" => Ok(Styles::Markdown),
            "re_structured_text" => Ok(Styles::ReStructuredText),
            "dots" => Ok(Styles::Dots),
            "ascii_rounded" => Ok(Styles::AsciiRounded),
            "blank" => Ok(Styles::Blank),
            "empty" => Ok(Styles::Empty),
            _ => Err(anyhow!("could not parse Style from {}", s)),
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ShortChannelState(pub ListpeerchannelsChannelsState);
impl Display for ShortChannelState {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self.0 {
            ListpeerchannelsChannelsState::OPENINGD => write!(f, "OPENING"),
            ListpeerchannelsChannelsState::CHANNELD_AWAITING_LOCKIN => write!(f, "AWAIT_LOCK"),
            ListpeerchannelsChannelsState::CHANNELD_NORMAL => write!(f, "OK"),
            ListpeerchannelsChannelsState::CHANNELD_SHUTTING_DOWN => write!(f, "SHUTTING_DOWN"),
            ListpeerchannelsChannelsState::CLOSINGD_SIGEXCHANGE => write!(f, "CLOSINGD_SIGEX"),
            ListpeerchannelsChannelsState::CLOSINGD_COMPLETE => write!(f, "CLOSINGD_DONE"),
            ListpeerchannelsChannelsState::AWAITING_UNILATERAL => write!(f, "AWAIT_UNILATERAL"),
            ListpeerchannelsChannelsState::FUNDING_SPEND_SEEN => write!(f, "FUNDING_SPEND"),
            ListpeerchannelsChannelsState::ONCHAIN => write!(f, "ONCHAIN"),
            ListpeerchannelsChannelsState::DUALOPEND_OPEN_INIT => write!(f, "DUAL_OPEN"),
            ListpeerchannelsChannelsState::DUALOPEND_OPEN_COMMITTED => write!(f, "DUAL_COMITTED"),
            ListpeerchannelsChannelsState::DUALOPEND_OPEN_COMMIT_READY => {
                write!(f, "DUAL_COMMIT_RDY")
            }
            ListpeerchannelsChannelsState::DUALOPEND_AWAITING_LOCKIN => write!(f, "DUAL_AWAIT"),
            ListpeerchannelsChannelsState::CHANNELD_AWAITING_SPLICE => write!(f, "AWAIT_SPLICE"),
        }
    }
}
impl FromStr for ShortChannelState {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "opening" => Ok(ShortChannelState(ListpeerchannelsChannelsState::OPENINGD)),
            "await_lock" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::CHANNELD_AWAITING_LOCKIN,
            )),
            "ok" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::CHANNELD_NORMAL,
            )),
            "shutting_down" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::CHANNELD_SHUTTING_DOWN,
            )),
            "closingd_sigex" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::CLOSINGD_SIGEXCHANGE,
            )),
            "closingd_done" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::CLOSINGD_COMPLETE,
            )),
            "await_unilateral" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::AWAITING_UNILATERAL,
            )),
            "funding_spend" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::FUNDING_SPEND_SEEN,
            )),
            "onchain" => Ok(ShortChannelState(ListpeerchannelsChannelsState::ONCHAIN)),
            "dual_open" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::DUALOPEND_OPEN_INIT,
            )),
            "dual_comitted" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::DUALOPEND_OPEN_COMMITTED,
            )),
            "dual_commit_rdy" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::DUALOPEND_OPEN_COMMIT_READY,
            )),
            "dual_await" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::DUALOPEND_AWAITING_LOCKIN,
            )),
            "await_splice" => Ok(ShortChannelState(
                ListpeerchannelsChannelsState::CHANNELD_AWAITING_SPLICE,
            )),
            _ => Err(anyhow!("could not parse State from {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        let opening = ShortChannelState(ListpeerchannelsChannelsState::OPENINGD).to_string();
        let await_lock =
            ShortChannelState(ListpeerchannelsChannelsState::CHANNELD_AWAITING_LOCKIN).to_string();
        let ok = ShortChannelState(ListpeerchannelsChannelsState::CHANNELD_NORMAL).to_string();
        let shutting_down =
            ShortChannelState(ListpeerchannelsChannelsState::CHANNELD_SHUTTING_DOWN).to_string();
        let closingd_sigex =
            ShortChannelState(ListpeerchannelsChannelsState::CLOSINGD_SIGEXCHANGE).to_string();
        let closingd_done =
            ShortChannelState(ListpeerchannelsChannelsState::CLOSINGD_COMPLETE).to_string();
        let await_unilateral =
            ShortChannelState(ListpeerchannelsChannelsState::AWAITING_UNILATERAL).to_string();
        let funding_spend =
            ShortChannelState(ListpeerchannelsChannelsState::FUNDING_SPEND_SEEN).to_string();
        let onchain = ShortChannelState(ListpeerchannelsChannelsState::ONCHAIN).to_string();
        let dual_open =
            ShortChannelState(ListpeerchannelsChannelsState::DUALOPEND_OPEN_INIT).to_string();
        let dual_comitted =
            ShortChannelState(ListpeerchannelsChannelsState::DUALOPEND_OPEN_COMMITTED).to_string();
        let dual_commit_rdy =
            ShortChannelState(ListpeerchannelsChannelsState::DUALOPEND_OPEN_COMMIT_READY)
                .to_string();
        let dual_await =
            ShortChannelState(ListpeerchannelsChannelsState::DUALOPEND_AWAITING_LOCKIN).to_string();
        let await_splice =
            ShortChannelState(ListpeerchannelsChannelsState::CHANNELD_AWAITING_SPLICE).to_string();

        assert_eq!(
            opening.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::OPENINGD
        );
        assert_eq!(
            await_lock.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::CHANNELD_AWAITING_LOCKIN
        );
        assert_eq!(
            ok.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::CHANNELD_NORMAL
        );
        assert_eq!(
            shutting_down.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::CHANNELD_SHUTTING_DOWN
        );
        assert_eq!(
            closingd_sigex.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::CLOSINGD_SIGEXCHANGE
        );
        assert_eq!(
            closingd_done.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::CLOSINGD_COMPLETE
        );
        assert_eq!(
            await_unilateral.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::AWAITING_UNILATERAL
        );
        assert_eq!(
            funding_spend.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::FUNDING_SPEND_SEEN
        );
        assert_eq!(
            onchain.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::ONCHAIN
        );
        assert_eq!(
            dual_open.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::DUALOPEND_OPEN_INIT
        );
        assert_eq!(
            dual_comitted.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::DUALOPEND_OPEN_COMMITTED
        );
        assert_eq!(
            dual_commit_rdy.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::DUALOPEND_OPEN_COMMIT_READY
        );
        assert_eq!(
            dual_await.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::DUALOPEND_AWAITING_LOCKIN
        );
        assert_eq!(
            await_splice.parse::<ShortChannelState>().unwrap().0,
            ListpeerchannelsChannelsState::CHANNELD_AWAITING_SPLICE
        );
    }
}
