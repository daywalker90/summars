use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::summars::PeerAvailability;
use anyhow::{anyhow, Error};
use cln_plugin::Plugin;
use cln_rpc::{
    model::*,
    primitives::{PublicKey, ShortChannelId},
    ClnRpc,
};
use parking_lot::Mutex;

use crate::{config::Config, summars::Summary};
pub mod config;
pub mod summars;
pub mod tasks;

pub const PLUGIN_NAME: &str = "summars";

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct PluginState {
    pub alias_map: Arc<Mutex<HashMap<String, String>>>,
    pub config: Arc<Mutex<Config>>,
    pub avail: Arc<Mutex<HashMap<String, PeerAvailability>>>,
}
impl PluginState {
    pub fn new() -> PluginState {
        PluginState {
            alias_map: Arc::new(Mutex::new(HashMap::new())),
            config: Arc::new(Mutex::new(Config::new())),
            avail: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub async fn list_funds(rpc_path: &PathBuf) -> Result<ListfundsResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let listfunds_request = rpc
        .call(Request::ListFunds(ListfundsRequest { spent: Some(false) }))
        .await
        .map_err(|e| anyhow!("Error calling list_funds: {}", e.to_string()))?;
    match listfunds_request {
        Response::ListFunds(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in list_funds: {:?}", e)),
    }
}

pub async fn list_peers(rpc_path: &PathBuf) -> Result<ListpeersResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let listpeers_request = rpc
        .call(Request::ListPeers(ListpeersRequest {
            id: None,
            level: None,
        }))
        .await
        .map_err(|e| anyhow!("Error calling list_peers: {}", e.to_string()))?;
    match listpeers_request {
        Response::ListPeers(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in list_peers: {:?}", e)),
    }
}

pub async fn list_nodes(rpc_path: &PathBuf, peer: &PublicKey) -> Result<ListnodesResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let listnodes_request = rpc
        .call(Request::ListNodes(ListnodesRequest { id: Some(*peer) }))
        .await
        .map_err(|e| anyhow!("Error calling list_nodes: {}", e.to_string()))?;
    match listnodes_request {
        Response::ListNodes(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in list_nodes: {:?}", e)),
    }
}

pub async fn get_info(rpc_path: &PathBuf) -> Result<GetinfoResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let getinfo_request = rpc
        .call(Request::Getinfo(GetinfoRequest {}))
        .await
        .map_err(|e| anyhow!("Error calling get_info: {}", e.to_string()))?;
    match getinfo_request {
        Response::Getinfo(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in get_info: {:?}", e)),
    }
}

pub async fn list_forwards(
    rpc_path: &PathBuf,
    status: Option<ListforwardsStatus>,
    in_channel: Option<ShortChannelId>,
    out_channel: Option<ShortChannelId>,
) -> Result<ListforwardsResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let listforwards_request = rpc
        .call(Request::ListForwards(ListforwardsRequest {
            status,
            in_channel,
            out_channel,
        }))
        .await
        .map_err(|e| anyhow!("Error calling list_forwards: {}", e.to_string()))?;
    match listforwards_request {
        Response::ListForwards(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in list_forwards: {:?}", e)),
    }
}

pub async fn list_pays(
    rpc_path: &PathBuf,
    status: Option<ListpaysStatus>,
) -> Result<ListpaysResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let listpays_request = rpc
        .call(Request::ListPays(ListpaysRequest {
            bolt11: None,
            payment_hash: None,
            status,
        }))
        .await
        .map_err(|e| anyhow!("Error calling list_pays: {}", e.to_string()))?;
    match listpays_request {
        Response::ListPays(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in list_pays: {:?}", e)),
    }
}

pub fn make_rpc_path(plugin: &Plugin<PluginState>) -> PathBuf {
    Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file)
}
