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
use sys_locale::get_locales;
use tabled::{settings::Style, Table, Tabled};

pub const NO_ALIAS_SET: &str = "NO_ALIAS_SET";
pub const NODE_GOSSIP_MISS: &str = "NODE_GOSSIP_MISS";

#[derive(Clone, Debug)]
pub struct Config {
    pub columns: Vec<String>,
    pub sort_by: String,
    pub exclude_channel_states: ExcludeStates,
    pub forwards: u64,
    pub forwards_columns: Vec<String>,
    pub forwards_filter_amt_msat: i64,
    pub forwards_filter_fee_msat: i64,
    pub forwards_alias: bool,
    pub pays: u64,
    pub pays_columns: Vec<String>,
    pub max_desc_length: i64,
    pub invoices: u64,
    pub invoices_columns: Vec<String>,
    pub max_label_length: i64,
    pub invoices_filter_amt_msat: i64,
    pub locale: Locale,
    pub refresh_alias: u64,
    pub max_alias_length: i64,
    pub availability_interval: u64,
    pub availability_window: u64,
    pub utf8: bool,
    pub style: Styles,
    pub flow_style: Styles,
    pub json: bool,
}
impl Config {
    pub fn new() -> Config {
        Config {
            columns: {
                Summary::FIELD_NAMES_AS_ARRAY
                    .into_iter()
                    .filter(|t| {
                        t != &"graph_sats"
                            && t != &"perc_us"
                            && t != &"total_sats"
                            && t != &"min_htlc"
                            && t != &"in_base"
                            && t != &"in_ppm"
                    })
                    .map(ToString::to_string)
                    .collect::<Vec<String>>()
            },
            sort_by: "SCID".to_string(),
            exclude_channel_states: ExcludeStates {
                channel_states: Vec::new(),
                channel_visibility: None,
                connection_status: None,
            },
            forwards: 0,
            forwards_columns: Forwards::FIELD_NAMES_AS_ARRAY
                .into_iter()
                .filter(|t| {
                    t != &"received_time"
                        && t != &"in_msats"
                        && t != &"out_msats"
                        && t != &"fee_sats"
                })
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
            forwards_filter_amt_msat: -1,
            forwards_filter_fee_msat: -1,
            forwards_alias: true,
            pays: 0,
            pays_columns: Pays::FIELD_NAMES_AS_ARRAY
                .into_iter()
                .filter(|t| {
                    t != &"description"
                        && t != &"preimage"
                        && t != &"sats_requested"
                        && t != &"msats_requested"
                        && t != &"msats_sent"
                        && t != &"fee_msats"
                })
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
            max_desc_length: 30,
            invoices: 0,
            invoices_columns: Invoices::FIELD_NAMES_AS_ARRAY
                .into_iter()
                .filter(|t| t != &"description" && t != &"preimage" && t != &"msats_received")
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
            max_label_length: 30,
            invoices_filter_amt_msat: -1,
            locale: {
                let mut valid_locale = None;
                for loc in get_locales() {
                    if let Ok(sl) = Locale::from_str(&loc) {
                        valid_locale = Some(sl);
                        break;
                    }
                }
                if let Some(vsl) = valid_locale {
                    vsl
                } else {
                    Locale::from_str("en-US").unwrap()
                }
            },
            refresh_alias: 24,
            max_alias_length: 20,
            availability_interval: 300,
            availability_window: 72,
            utf8: true,
            style: Styles::Psql,

            flow_style: Styles::Blank,

            json: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExcludeStates {
    pub channel_states: Vec<ShortChannelState>,
    pub channel_visibility: Option<ChannelVisibility>,
    pub connection_status: Option<ConnectionStatus>,
}

#[derive(Clone, Debug)]
pub enum ConnectionStatus {
    Online,
    Offline,
}

#[derive(Clone, Debug)]
pub enum ChannelVisibility {
    Private,
    Public,
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

#[derive(Debug, Tabled, FieldNamesAsArray, Serialize)]
#[tabled(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Summary {
    #[serde(skip_serializing)]
    pub graph_sats: String,
    pub out_sats: u64,
    pub in_sats: u64,
    pub total_sats: u64,
    #[tabled(skip)]
    #[serde(skip_serializing)]
    #[field_names_as_array(skip)]
    pub scid_raw: ShortChannelId,
    pub scid: String,
    pub max_htlc: u64,
    pub min_htlc: u64,
    #[serde(skip_serializing)]
    pub flag: String,
    #[tabled(skip)]
    #[field_names_as_array(skip)]
    pub private: bool,
    #[tabled(skip)]
    #[field_names_as_array(skip)]
    pub offline: bool,
    pub base: u64,
    #[serde(skip_serializing)]
    pub in_base: String,
    pub ppm: u32,
    #[serde(skip_serializing)]
    pub in_ppm: String,
    pub alias: String,
    pub peer_id: PublicKey,
    pub uptime: f64,
    pub htlcs: usize,
    pub state: String,
    pub perc_us: f64,
}

#[derive(Debug, Tabled, FieldNamesAsArray, Serialize)]
pub struct Forwards {
    #[tabled(skip)]
    pub received_time: u64,
    #[tabled(rename = "received_time")]
    #[serde(skip_serializing)]
    #[field_names_as_array(skip)]
    pub received_time_str: String,
    #[tabled(skip)]
    pub resolved_time: u64,
    #[tabled(rename = "resolved_time")]
    #[serde(skip_serializing)]
    #[field_names_as_array(skip)]
    pub resolved_time_str: String,
    #[tabled(rename = "in_channel")]
    #[serde(skip_serializing)]
    #[field_names_as_array(skip)]
    pub in_channel_alias: String,
    #[tabled(rename = "out_channel")]
    #[serde(skip_serializing)]
    #[field_names_as_array(skip)]
    pub out_channel_alias: String,
    #[tabled(skip)]
    pub in_channel: ShortChannelId,
    #[tabled(skip)]
    pub out_channel: ShortChannelId,
    pub in_msats: u64,
    #[serde(skip_serializing)]
    pub in_sats: u64,
    pub out_msats: u64,
    #[serde(skip_serializing)]
    pub out_sats: u64,
    pub fee_msats: u64,
    #[serde(skip_serializing)]
    pub fee_sats: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ForwardsFilterStats {
    pub filter_amt_sum_msat: u64,
    pub filter_fee_sum_msat: u64,
    pub filter_count: u64,
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

#[derive(Debug, Tabled, FieldNamesAsArray, Serialize)]
pub struct Pays {
    #[tabled(skip)]
    pub completed_at: u64,
    #[tabled(rename = "completed_at")]
    #[serde(skip_serializing)]
    #[field_names_as_array(skip)]
    pub completed_at_str: String,
    pub payment_hash: String,
    pub msats_requested: u64,
    #[serde(skip_serializing)]
    pub sats_requested: u64,
    pub msats_sent: u64,
    #[serde(skip_serializing)]
    pub sats_sent: u64,
    pub fee_msats: u64,
    #[serde(skip_serializing)]
    pub fee_sats: u64,
    pub destination: String,
    #[serde(skip_serializing)]
    pub description: String,
    pub preimage: String,
}

#[derive(Debug, Tabled, FieldNamesAsArray, Serialize)]
pub struct Invoices {
    #[tabled(skip)]
    pub paid_at: u64,
    #[tabled(rename = "paid_at")]
    #[serde(skip_serializing)]
    #[field_names_as_array(skip)]
    pub paid_at_str: String,
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub msats_received: u64,
    #[serde(skip_serializing)]
    pub sats_received: u64,
    pub payment_hash: String,
    pub preimage: String,
}

#[derive(Debug, Clone, Default)]
pub struct InvoicesFilterStats {
    pub filter_amt_sum_msat: u64,
    pub filter_count: u64,
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
pub struct GraphCharset {
    pub double_left: String,
    pub left: String,
    pub bar: String,
    pub mid: String,
    pub right: String,
    pub double_right: String,
    pub empty: String,
}
impl GraphCharset {
    pub fn new_utf8() -> GraphCharset {
        GraphCharset {
            double_left: "╟".to_string(),
            left: "├".to_string(),
            bar: "─".to_string(),
            mid: "┼".to_string(),
            right: "┤".to_string(),
            double_right: "╢".to_string(),
            empty: "║".to_string(),
        }
    }
    pub fn new_ascii() -> GraphCharset {
        GraphCharset {
            double_left: "#".to_string(),
            left: "[".to_string(),
            bar: "-".to_string(),
            mid: "+".to_string(),
            right: "]".to_string(),
            double_right: "#".to_string(),
            empty: "|".to_string(),
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
