use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    str::FromStr,
    sync::Arc,
};

use anyhow::anyhow;
use cln_plugin::Error;
use cln_rpc::primitives::{ChannelState, PublicKey, ShortChannelId};
use icu_locale::Locale;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sys_locale::get_locales;
use tabled::{derive::display, settings::Style, Table, Tabled};
#[cfg(feature = "hold")]
use tonic::transport::Channel;

#[cfg(feature = "hold")]
use crate::hold::hold_client::HoldClient;
use crate::impl_table_column;

pub const NO_ALIAS_SET: &str = "NO_ALIAS_SET";
pub const NODE_GOSSIP_MISS: &str = "NODE_GOSSIP_MISS";
pub const MISSING_VALUE: &str = "N/A";
pub const PAGE_SIZE: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opt {
    Columns,
    ClosedChannels,
    ClosedChannelsColumns,
    SortBy,
    ExcludeChannelStates,
    Forwards,
    ForwardsLimit,
    ForwardsColumns,
    ForwardsFilterAmt,
    ForwardsFilterFee,
    Pays,
    PaysLimit,
    PaysColumns,
    MaxDescLength,
    Invoices,
    InvoicesLimit,
    InvoicesColumns,
    MaxLabelLength,
    InvoicesFilterAmt,
    Locale,
    RefreshAlias,
    MaxAliasLength,
    AvailabilityInterval,
    AvailabilityWindow,
    Utf8,
    Style,
    FlowStyle,
    Json,
}

impl Opt {
    pub fn as_key(self) -> &'static str {
        match self {
            Opt::Columns => "summars-columns",
            Opt::ClosedChannels => "summars-closed-channels",
            Opt::ClosedChannelsColumns => "summars-closed-channels-columns",
            Opt::SortBy => "summars-sort-by",
            Opt::ExcludeChannelStates => "summars-exclude-states",
            Opt::Forwards => "summars-forwards",
            Opt::ForwardsLimit => "summars-forwards-limit",
            Opt::ForwardsColumns => "summars-forwards-columns",
            Opt::ForwardsFilterAmt => "summars-forwards-filter-amount-msat",
            Opt::ForwardsFilterFee => "summars-forwards-filter-fee-msat",
            Opt::Pays => "summars-pays",
            Opt::PaysLimit => "summars-pays-limit",
            Opt::PaysColumns => "summars-pays-columns",
            Opt::MaxDescLength => "summars-max-description-length",
            Opt::Invoices => "summars-invoices",
            Opt::InvoicesLimit => "summars-invoices-limit",
            Opt::InvoicesColumns => "summars-invoices-columns",
            Opt::MaxLabelLength => "summars-max-label-length",
            Opt::InvoicesFilterAmt => "summars-invoices-filter-amount-msat",
            Opt::Locale => "summars-locale",
            Opt::RefreshAlias => "summars-refresh-alias",
            Opt::MaxAliasLength => "summars-max-alias-length",
            Opt::AvailabilityInterval => "summars-availability-interval",
            Opt::AvailabilityWindow => "summars-availability-window",
            Opt::Utf8 => "summars-utf8",
            Opt::Style => "summars-style",
            Opt::FlowStyle => "summars-flow-style",
            Opt::Json => "summars-json",
        }
    }

    pub fn from_key(s: &str) -> Result<Opt, Error> {
        match s {
            "summars-columns" => Ok(Opt::Columns),
            "summars-closed-channels" => Ok(Opt::ClosedChannels),
            "summars-closed-channels-columns" => Ok(Opt::ClosedChannelsColumns),
            "summars-sort-by" => Ok(Opt::SortBy),
            "summars-exclude-states" => Ok(Opt::ExcludeChannelStates),
            "summars-forwards" => Ok(Opt::Forwards),
            "summars-forwards-limit" => Ok(Opt::ForwardsLimit),
            "summars-forwards-columns" => Ok(Opt::ForwardsColumns),
            "summars-forwards-filter-amount-msat" => Ok(Opt::ForwardsFilterAmt),
            "summars-forwards-filter-fee-msat" => Ok(Opt::ForwardsFilterFee),
            "summars-pays" => Ok(Opt::Pays),
            "summars-pays-limit" => Ok(Opt::PaysLimit),
            "summars-pays-columns" => Ok(Opt::PaysColumns),
            "summars-max-description-length" => Ok(Opt::MaxDescLength),
            "summars-invoices" => Ok(Opt::Invoices),
            "summars-invoices-limit" => Ok(Opt::InvoicesLimit),
            "summars-invoices-columns" => Ok(Opt::InvoicesColumns),
            "summars-max-label-length" => Ok(Opt::MaxLabelLength),
            "summars-invoices-filter-amount-msat" => Ok(Opt::InvoicesFilterAmt),
            "summars-locale" => Ok(Opt::Locale),
            "summars-refresh-alias" => Ok(Opt::RefreshAlias),
            "summars-max-alias-length" => Ok(Opt::MaxAliasLength),
            "summars-availability-interval" => Ok(Opt::AvailabilityInterval),
            "summars-availability-window" => Ok(Opt::AvailabilityWindow),
            "summars-utf8" => Ok(Opt::Utf8),
            "summars-style" => Ok(Opt::Style),
            "summars-flow-style" => Ok(Opt::FlowStyle),
            "summars-json" => Ok(Opt::Json),
            _ => Err(anyhow!("Unknown option: {s}")),
        }
    }

    pub const ALL: &[Opt] = &[
        Opt::Columns,
        Opt::ClosedChannels,
        Opt::ClosedChannelsColumns,
        Opt::SortBy,
        Opt::ExcludeChannelStates,
        Opt::Forwards,
        Opt::ForwardsLimit,
        Opt::ForwardsColumns,
        Opt::ForwardsFilterAmt,
        Opt::ForwardsFilterFee,
        Opt::Pays,
        Opt::PaysLimit,
        Opt::PaysColumns,
        Opt::MaxDescLength,
        Opt::Invoices,
        Opt::InvoicesLimit,
        Opt::InvoicesColumns,
        Opt::MaxLabelLength,
        Opt::InvoicesFilterAmt,
        Opt::Locale,
        Opt::RefreshAlias,
        Opt::MaxAliasLength,
        Opt::AvailabilityInterval,
        Opt::AvailabilityWindow,
        Opt::Utf8,
        Opt::Style,
        Opt::FlowStyle,
        Opt::Json,
    ];

    pub fn iter() -> impl Iterator<Item = Opt> + 'static {
        Self::ALL.iter().copied()
    }

    pub fn is_internal(self) -> bool {
        matches!(
            self,
            Opt::RefreshAlias | Opt::AvailabilityInterval | Opt::AvailabilityWindow
        )
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub columns: Vec<SummaryColumns>,
    pub closed_channels: usize,
    pub closed_channels_columns: Vec<ClosedChannelsColumns>,
    pub sort_by: SummaryColumns,
    pub sort_reverse: bool,
    pub exclude_channel_states: ExcludeStates,
    pub forwards: u64,
    pub forwards_limit: usize,
    pub forwards_columns: Vec<ForwardsColumns>,
    pub forwards_filter_amt_msat: Option<u64>,
    pub forwards_filter_fee_msat: Option<u64>,
    pub pays: u64,
    pub pays_limit: usize,
    pub pays_columns: Vec<PaysColumns>,
    pub max_desc_length: i64,
    pub invoices: u64,
    pub invoices_limit: usize,
    pub invoices_columns: Vec<InvoicesColumns>,
    pub max_label_length: i64,
    pub invoices_filter_amt_msat: Option<u64>,
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
            columns: { SummaryColumns::default_columns() },
            closed_channels: 0,
            closed_channels_columns: { ClosedChannelsColumns::default_columns() },
            sort_by: SummaryColumns::SCID,
            sort_reverse: false,
            exclude_channel_states: ExcludeStates {
                channel_states: Vec::new(),
                channel_visibility: None,
                connection_status: None,
            },
            forwards: 0,
            forwards_limit: 0,
            forwards_columns: ForwardsColumns::default_columns(),
            forwards_filter_amt_msat: None,
            forwards_filter_fee_msat: None,
            pays: 0,
            pays_limit: 0,
            pays_columns: PaysColumns::default_columns(),
            max_desc_length: 30,
            invoices: 0,
            invoices_limit: 0,
            invoices_columns: InvoicesColumns::default_columns(),
            max_label_length: 30,
            invoices_filter_amt_msat: None,
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
    #[cfg(feature = "hold")]
    pub hold_client: Arc<Mutex<Option<HoldClient<Channel>>>>,
    #[cfg(feature = "hold")]
    pub hold_pagination_helper: Arc<Mutex<HoldInvoicePageHelper>>,
}
impl PluginState {
    pub fn new() -> PluginState {
        PluginState {
            alias_map: Arc::new(Mutex::new(BTreeMap::new())),
            config: Arc::new(Mutex::new(Config::new())),
            avail: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(feature = "hold")]
            hold_client: Arc::new(Mutex::new(None)),
            #[cfg(feature = "hold")]
            hold_pagination_helper: Arc::new(Mutex::new(HoldInvoicePageHelper::default())),
        }
    }
}

#[cfg(feature = "hold")]
#[derive(Debug, Clone, Copy)]
pub struct HoldInvoicePageHelper {
    pub first_index: i64,
    pub last_window: u64,
}
#[cfg(feature = "hold")]
impl Default for HoldInvoicePageHelper {
    fn default() -> Self {
        Self {
            first_index: 1,
            last_window: 0,
        }
    }
}

#[derive(Debug)]
pub struct FullNodeData {
    pub node_summary: NodeSummary,
    pub channels: Vec<Summary>,
    pub closed_channels: Vec<ClosedChannels>,
    pub forwards: Vec<Forwards>,
    pub forwards_filter_stats: ForwardsFilterStats,
    pub pays: Vec<Pays>,
    pub invoices: Vec<Invoices>,
    pub invoices_filter_stats: InvoicesFilterStats,
    pub totals: Totals,
    pub graph_max_chan_side_msat: u64,
    pub cln_version: String,
    pub my_pubkey: PublicKey,
}
impl FullNodeData {
    pub fn new(
        my_pubkey: PublicKey,
        cln_version: String,
        graph_max_chan_side_msat: u64,
    ) -> FullNodeData {
        FullNodeData {
            node_summary: NodeSummary::default(),
            channels: Vec::new(),
            closed_channels: Vec::new(),
            forwards: Vec::new(),
            forwards_filter_stats: ForwardsFilterStats::default(),
            pays: Vec::new(),
            invoices: Vec::new(),
            invoices_filter_stats: InvoicesFilterStats::default(),
            totals: Totals::default(),
            graph_max_chan_side_msat,
            cln_version,
            my_pubkey,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Copy, Clone)]
pub struct PeerAvailability {
    pub count: u64,
    pub connected: bool,
    pub avail: f64,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct NodeSummary {
    pub channel_count: u32,
    pub num_connected: u32,
    pub num_gossipers: usize,
    pub avail_in: u64,
    pub avail_out: u64,
    pub filter_count: u32,
}

pub trait TableColumn: Copy + Eq + std::hash::Hash + Display + FromStr + 'static {
    const NUMERICAL: &[Self];
    const OPTIONAL_NUMERICAL: &[Self];
    fn default_columns() -> Vec<Self>;
    fn parse_column(input: &str) -> Result<Self, Error>;
    fn parse_columns(input: &str) -> Result<Vec<Self>, Error>;
    // fn all_list_string() -> String;
    fn to_list_string(columns: &[Self]) -> String;
}

#[derive(Debug, Tabled, Serialize)]
#[tabled(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Summary {
    #[serde(skip_serializing)]
    pub graph_sats: String,
    pub out_sats: u64,
    pub in_sats: u64,
    pub total_sats: u64,
    #[tabled(skip)]
    #[serde(skip_serializing)]
    pub scid_raw: ShortChannelId,
    pub scid: String,
    pub max_htlc: u64,
    pub min_htlc: u64,
    #[serde(skip_serializing)]
    pub flag: String,
    #[tabled(skip)]
    pub private: bool,
    #[tabled(skip)]
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
    pub ping: u64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter, strum::EnumString, strum::Display,
)]
#[allow(non_camel_case_types)]
#[strum(ascii_case_insensitive)]
#[allow(clippy::upper_case_acronyms)]
pub enum SummaryColumns {
    GRAPH_SATS,
    OUT_SATS,
    IN_SATS,
    TOTAL_SATS,
    SCID,
    MAX_HTLC,
    MIN_HTLC,
    FLAG,
    BASE,
    IN_BASE,
    PPM,
    IN_PPM,
    ALIAS,
    PEER_ID,
    UPTIME,
    HTLCS,
    STATE,
    PERC_US,
    PING,
}
impl_table_column!(
    SummaryColumns,
    env_var = Opt::Columns.as_key(),
    exclude_default = [GRAPH_SATS, PERC_US, TOTAL_SATS, MIN_HTLC, IN_BASE, IN_PPM, PING],
    numerical = [OUT_SATS, IN_SATS, TOTAL_SATS, MIN_HTLC, MAX_HTLC, BASE, PPM],
    optional_numerical = [IN_BASE, IN_PPM],
);

#[derive(Debug, Tabled, Serialize)]
#[tabled(display(Option, "display::option", "N/A"))]
#[tabled(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct ClosedChannels {
    pub out_sats: u64,
    pub in_sats: u64,
    pub total_sats: u64,
    pub scid: Option<String>,
    #[serde(skip_serializing)]
    pub flag: String,
    #[tabled(skip)]
    pub private: bool,
    pub alias: String,
    pub peer_id: Option<String>,
    pub htlcs_sent: u64,
    pub close_cause: String,
    pub last_connect: Option<u64>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter, strum::EnumString, strum::Display,
)]
#[allow(non_camel_case_types)]
#[strum(ascii_case_insensitive)]
#[allow(clippy::upper_case_acronyms)]
pub enum ClosedChannelsColumns {
    LAST_CONNECT,
    OUT_SATS,
    IN_SATS,
    TOTAL_SATS,
    SCID,
    FLAG,
    ALIAS,
    PEER_ID,
    HTLCS_SENT,
    CLOSE_CAUSE,
}
impl_table_column!(
    ClosedChannelsColumns,
    env_var = Opt::ClosedChannelsColumns.as_key(),
    exclude_default = [TOTAL_SATS],
    numerical = [OUT_SATS, IN_SATS, TOTAL_SATS],
    optional_numerical = [],
);

#[derive(Debug, Tabled, Serialize)]
pub struct Forwards {
    #[tabled(skip)]
    pub received_time: u64,
    #[tabled(rename = "received_time")]
    #[serde(skip_serializing)]
    pub received_time_str: String,
    #[tabled(skip)]
    pub resolved_time: u64,
    #[tabled(rename = "resolved_time")]
    #[serde(skip_serializing)]
    pub resolved_time_str: String,
    pub in_alias: String,
    pub out_alias: String,
    pub in_channel: ShortChannelId,
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
    pub eff_fee_ppm: u32,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter, strum::EnumString, strum::Display,
)]
#[allow(non_camel_case_types)]
#[strum(ascii_case_insensitive)]
pub enum ForwardsColumns {
    received_time,
    resolved_time,
    in_alias,
    out_alias,
    in_channel,
    out_channel,
    in_msats,
    out_msats,
    in_sats,
    out_sats,
    fee_msats,
    fee_sats,
    eff_fee_ppm,
}
impl_table_column!(
    ForwardsColumns,
    env_var = Opt::ForwardsColumns.as_key(),
    exclude_default = [
        received_time,
        in_msats,
        out_msats,
        fee_sats,
        eff_fee_ppm,
        in_channel,
        out_channel
    ],
    numerical = [
        in_sats,
        in_msats,
        out_sats,
        out_msats,
        fee_sats,
        fee_msats,
        eff_fee_ppm
    ],
    optional_numerical = [],
);

#[derive(Debug, Clone, Default)]
pub struct ForwardsFilterStats {
    pub amt_sum_msat: u64,
    pub fee_sum_msat: u64,
    pub count: u64,
}

#[derive(Debug, Tabled, Serialize)]
pub struct Pays {
    pub completed_at: u64,
    pub payment_hash: String,
    #[tabled(display = "fmt_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msats_requested: Option<u64>,
    #[serde(skip_serializing)]
    #[tabled(display = "fmt_option")]
    pub sats_requested: Option<u64>,
    pub msats_sent: u64,
    #[serde(skip_serializing)]
    pub sats_sent: u64,
    #[tabled(display = "fmt_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_msats: Option<u64>,
    #[tabled(display = "fmt_option")]
    #[serde(skip_serializing)]
    pub fee_sats: Option<u64>,
    #[tabled(display = "fmt_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[tabled(display = "fmt_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub preimage: String,
    #[serde(skip_serializing)]
    #[tabled(skip)]
    pub description_status: DescriptionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptionStatus {
    Processed,
    Bolt11,
    Bolt12,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter, strum::EnumString, strum::Display,
)]
#[allow(non_camel_case_types)]
#[strum(ascii_case_insensitive)]
pub enum PaysColumns {
    completed_at,
    payment_hash,
    msats_requested,
    sats_requested,
    msats_sent,
    sats_sent,
    fee_msats,
    fee_sats,
    destination,
    description,
    preimage,
}
impl_table_column!(
    PaysColumns,
    env_var = Opt::PaysColumns.as_key(),
    exclude_default = [
        description,
        preimage,
        sats_requested,
        msats_requested,
        msats_sent,
        fee_msats,
    ],
    numerical = [sats_sent, msats_sent],
    optional_numerical = [sats_requested, msats_requested, fee_sats, fee_msats],
);

#[allow(clippy::ref_option)]
fn fmt_option<T: Display>(o: &Option<T>) -> String {
    match o {
        Some(s) => format!("{s}"),
        None => MISSING_VALUE.to_owned(),
    }
}

#[derive(Debug, Tabled, Serialize)]
pub struct Invoices {
    #[tabled(skip)]
    pub paid_at: u64,
    #[tabled(rename = "paid_at")]
    #[serde(skip_serializing)]
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

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter, strum::EnumString, strum::Display,
)]
#[allow(non_camel_case_types)]
#[strum(ascii_case_insensitive)]
pub enum InvoicesColumns {
    paid_at,
    label,
    description,
    msats_received,
    sats_received,
    payment_hash,
    preimage,
}
impl_table_column!(
    InvoicesColumns,
    env_var = Opt::InvoicesColumns.as_key(),
    exclude_default = [description, preimage, msats_received],
    numerical = [sats_received, msats_received],
    optional_numerical = [],
);

#[derive(Debug, Clone, Default)]
pub struct InvoicesFilterStats {
    pub filter_amt_sum_msat: u64,
    pub filter_count: u64,
}

#[derive(Debug, Serialize, Default, Clone, Copy)]
pub struct Totals {
    pub pays: PaysTotals,
    pub invoices: InvoicesTotals,
    pub forwards: ForwardsTotals,
}

#[derive(Debug, Serialize, Default, Clone, Copy)]
pub struct PaysTotals {
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_msat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_sent_msat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fees_msat: Option<u64>,
    pub self_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_amount_msat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_amount_sent_msat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_fees_msat: Option<u64>,
}

#[derive(Debug, Serialize, Default, Clone, Copy)]
pub struct InvoicesTotals {
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_received_msat: Option<u64>,
}

#[derive(Debug, Serialize, Default, Clone, Copy)]
pub struct ForwardsTotals {
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_in_msat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_out_msat: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fees_msat: Option<u64>,
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
    pub fn apply<'a>(&'a self, table: &'a mut Table) -> &'a mut Table {
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
            _ => Err(anyhow!("could not parse Style from {s}")),
        }
    }
}
impl Display for Styles {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Styles::Ascii => write!(f, "ascii"),
            Styles::Modern => write!(f, "modern"),
            Styles::Sharp => write!(f, "sharp"),
            Styles::Rounded => write!(f, "rounded"),
            Styles::Extended => write!(f, "extended"),
            Styles::Psql => write!(f, "psql"),
            Styles::Markdown => write!(f, "markdown"),
            Styles::ReStructuredText => write!(f, "re_structured_text"),
            Styles::Dots => write!(f, "dots"),
            Styles::AsciiRounded => write!(f, "ascii_rounded"),
            Styles::Blank => write!(f, "blank"),
            Styles::Empty => write!(f, "empty"),
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ShortChannelState(pub ChannelState);
impl Display for ShortChannelState {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self.0 {
            ChannelState::OPENINGD => write!(f, "OPENING"),
            ChannelState::CHANNELD_AWAITING_LOCKIN => write!(f, "AWAIT_LOCK"),
            ChannelState::CHANNELD_NORMAL => write!(f, "OK"),
            ChannelState::CHANNELD_SHUTTING_DOWN => write!(f, "SHUTTING_DOWN"),
            ChannelState::CLOSINGD_SIGEXCHANGE => write!(f, "CLOSINGD_SIGEX"),
            ChannelState::CLOSINGD_COMPLETE => write!(f, "CLOSINGD_DONE"),
            ChannelState::AWAITING_UNILATERAL => write!(f, "AWAIT_UNILATERAL"),
            ChannelState::FUNDING_SPEND_SEEN => write!(f, "FUNDING_SPEND"),
            ChannelState::ONCHAIN => write!(f, "ONCHAIN"),
            ChannelState::DUALOPEND_OPEN_INIT => write!(f, "DUAL_OPEN"),
            ChannelState::DUALOPEND_OPEN_COMMITTED => write!(f, "DUAL_COMITTED"),
            ChannelState::DUALOPEND_OPEN_COMMIT_READY => {
                write!(f, "DUAL_COMMIT_RDY")
            }
            ChannelState::DUALOPEND_AWAITING_LOCKIN => write!(f, "DUAL_AWAIT"),
            ChannelState::CHANNELD_AWAITING_SPLICE => write!(f, "AWAIT_SPLICE"),
        }
    }
}
impl FromStr for ShortChannelState {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "opening" => Ok(ShortChannelState(ChannelState::OPENINGD)),
            "await_lock" => Ok(ShortChannelState(ChannelState::CHANNELD_AWAITING_LOCKIN)),
            "ok" => Ok(ShortChannelState(ChannelState::CHANNELD_NORMAL)),
            "shutting_down" => Ok(ShortChannelState(ChannelState::CHANNELD_SHUTTING_DOWN)),
            "closingd_sigex" => Ok(ShortChannelState(ChannelState::CLOSINGD_SIGEXCHANGE)),
            "closingd_done" => Ok(ShortChannelState(ChannelState::CLOSINGD_COMPLETE)),
            "await_unilateral" => Ok(ShortChannelState(ChannelState::AWAITING_UNILATERAL)),
            "funding_spend" => Ok(ShortChannelState(ChannelState::FUNDING_SPEND_SEEN)),
            "onchain" => Ok(ShortChannelState(ChannelState::ONCHAIN)),
            "dual_open" => Ok(ShortChannelState(ChannelState::DUALOPEND_OPEN_INIT)),
            "dual_comitted" => Ok(ShortChannelState(ChannelState::DUALOPEND_OPEN_COMMITTED)),
            "dual_commit_rdy" => Ok(ShortChannelState(ChannelState::DUALOPEND_OPEN_COMMIT_READY)),
            "dual_await" => Ok(ShortChannelState(ChannelState::DUALOPEND_AWAITING_LOCKIN)),
            "await_splice" => Ok(ShortChannelState(ChannelState::CHANNELD_AWAITING_SPLICE)),
            _ => Err(anyhow!("could not parse State from {s}")),
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
            double_left: "╟".to_owned(),
            left: "├".to_owned(),
            bar: "─".to_owned(),
            mid: "┼".to_owned(),
            right: "┤".to_owned(),
            double_right: "╢".to_owned(),
            empty: "║".to_owned(),
        }
    }
    pub fn new_ascii() -> GraphCharset {
        GraphCharset {
            double_left: "#".to_owned(),
            left: "[".to_owned(),
            bar: "-".to_owned(),
            mid: "+".to_owned(),
            right: "]".to_owned(),
            double_right: "#".to_owned(),
            empty: "|".to_owned(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        let opening = ShortChannelState(ChannelState::OPENINGD).to_string();
        let await_lock = ShortChannelState(ChannelState::CHANNELD_AWAITING_LOCKIN).to_string();
        let ok = ShortChannelState(ChannelState::CHANNELD_NORMAL).to_string();
        let shutting_down = ShortChannelState(ChannelState::CHANNELD_SHUTTING_DOWN).to_string();
        let closingd_sigex = ShortChannelState(ChannelState::CLOSINGD_SIGEXCHANGE).to_string();
        let closingd_done = ShortChannelState(ChannelState::CLOSINGD_COMPLETE).to_string();
        let await_unilateral = ShortChannelState(ChannelState::AWAITING_UNILATERAL).to_string();
        let funding_spend = ShortChannelState(ChannelState::FUNDING_SPEND_SEEN).to_string();
        let onchain = ShortChannelState(ChannelState::ONCHAIN).to_string();
        let dual_open = ShortChannelState(ChannelState::DUALOPEND_OPEN_INIT).to_string();
        let dual_comitted = ShortChannelState(ChannelState::DUALOPEND_OPEN_COMMITTED).to_string();
        let dual_commit_rdy =
            ShortChannelState(ChannelState::DUALOPEND_OPEN_COMMIT_READY).to_string();
        let dual_await = ShortChannelState(ChannelState::DUALOPEND_AWAITING_LOCKIN).to_string();
        let await_splice = ShortChannelState(ChannelState::CHANNELD_AWAITING_SPLICE).to_string();

        assert_eq!(
            opening.parse::<ShortChannelState>().unwrap().0,
            ChannelState::OPENINGD
        );
        assert_eq!(
            await_lock.parse::<ShortChannelState>().unwrap().0,
            ChannelState::CHANNELD_AWAITING_LOCKIN
        );
        assert_eq!(
            ok.parse::<ShortChannelState>().unwrap().0,
            ChannelState::CHANNELD_NORMAL
        );
        assert_eq!(
            shutting_down.parse::<ShortChannelState>().unwrap().0,
            ChannelState::CHANNELD_SHUTTING_DOWN
        );
        assert_eq!(
            closingd_sigex.parse::<ShortChannelState>().unwrap().0,
            ChannelState::CLOSINGD_SIGEXCHANGE
        );
        assert_eq!(
            closingd_done.parse::<ShortChannelState>().unwrap().0,
            ChannelState::CLOSINGD_COMPLETE
        );
        assert_eq!(
            await_unilateral.parse::<ShortChannelState>().unwrap().0,
            ChannelState::AWAITING_UNILATERAL
        );
        assert_eq!(
            funding_spend.parse::<ShortChannelState>().unwrap().0,
            ChannelState::FUNDING_SPEND_SEEN
        );
        assert_eq!(
            onchain.parse::<ShortChannelState>().unwrap().0,
            ChannelState::ONCHAIN
        );
        assert_eq!(
            dual_open.parse::<ShortChannelState>().unwrap().0,
            ChannelState::DUALOPEND_OPEN_INIT
        );
        assert_eq!(
            dual_comitted.parse::<ShortChannelState>().unwrap().0,
            ChannelState::DUALOPEND_OPEN_COMMITTED
        );
        assert_eq!(
            dual_commit_rdy.parse::<ShortChannelState>().unwrap().0,
            ChannelState::DUALOPEND_OPEN_COMMIT_READY
        );
        assert_eq!(
            dual_await.parse::<ShortChannelState>().unwrap().0,
            ChannelState::DUALOPEND_AWAITING_LOCKIN
        );
        assert_eq!(
            await_splice.parse::<ShortChannelState>().unwrap().0,
            ChannelState::CHANNELD_AWAITING_SPLICE
        );
    }

    #[test]
    fn test_opt_from_key() {
        for opt in Opt::iter() {
            assert_eq!(Opt::from_key(opt.as_key()).unwrap(), opt);
        }
    }
}
