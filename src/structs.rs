use std::{collections::BTreeMap, sync::Arc};

use cln_rpc::primitives::{PublicKey, ShortChannelId};
use num_format::Locale;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::Tabled;

pub const PLUGIN_NAME: &str = "summars";
pub const NO_ALIAS_SET: &str = "NO_ALIAS_SET";
pub const NODE_GOSSIP_MISS: &str = "NODE_GOSSIP_MISS";

#[derive(Clone, Debug)]
pub struct Config {
    pub columns: (String, Vec<String>),
    pub sort_by: (String, String),
    pub forwards: (String, u64),
    pub forward_alias: (String, bool),
    pub pays: (String, u64),
    pub invoices: (String, u64),
    pub locale: (String, Locale),
    pub refresh_alias: (String, u64),
    pub max_alias_length: (String, u64),
    pub availability_interval: (String, u64),
    pub availability_window: (String, u64),
    pub utf8: (String, bool),
}
impl Config {
    pub fn new() -> Config {
        Config {
            columns: (
                PLUGIN_NAME.to_string() + "-columns",
                Summary::get_field_names(),
            ),
            sort_by: (PLUGIN_NAME.to_string() + "-sort-by", "SCID".to_string()),
            forwards: (PLUGIN_NAME.to_string() + "-forwards", 0),
            forward_alias: (PLUGIN_NAME.to_string() + "-forward-alias", true),
            pays: (PLUGIN_NAME.to_string() + "-pays", 0),
            invoices: (PLUGIN_NAME.to_string() + "-invoices", 0),
            locale: (PLUGIN_NAME.to_string() + "-locale", Locale::en),
            refresh_alias: (PLUGIN_NAME.to_string() + "-refresh-alias", 24),
            max_alias_length: (PLUGIN_NAME.to_string() + "-max-alias-length", 20),
            availability_interval: (PLUGIN_NAME.to_string() + "-availability-interval", 300),
            availability_window: (PLUGIN_NAME.to_string() + "-availability-window", 72),
            utf8: (PLUGIN_NAME.to_string() + "-utf8", true),
        }
    }
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
    pub fn get_field_names() -> Vec<String> {
        Summary::FIELD_NAMES_AS_ARRAY
            .into_iter()
            .map(String::from)
            .collect()
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
