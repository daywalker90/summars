use std::path::PathBuf;

use anyhow::{anyhow, Error};

use cln_rpc::{
    model::requests::*,
    model::responses::*,
    primitives::{PublicKey, ShortChannelId},
    ClnRpc, Request, Response,
};

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

pub async fn list_peer_channels(rpc_path: &PathBuf) -> Result<ListpeerchannelsResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let list_peer_channels = rpc
        .call(Request::ListPeerChannels(ListpeerchannelsRequest {
            id: None,
        }))
        .await
        .map_err(|e| anyhow!("Error calling list_peer_channels: {}", e.to_string()))?;
    match list_peer_channels {
        Response::ListPeerChannels(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in list_peer_channels: {:?}", e)),
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
    index: Option<ListforwardsIndex>,
    start: Option<u64>,
    limit: Option<u32>,
) -> Result<ListforwardsResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let listforwards_request = rpc
        .call(Request::ListForwards(ListforwardsRequest {
            status,
            in_channel,
            out_channel,
            index,
            start,
            limit,
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

pub async fn list_invoices(
    rpc_path: &PathBuf,
    label: Option<String>,
    payment_hash: Option<String>,
    index: Option<ListinvoicesIndex>,
    start: Option<u64>,
    limit: Option<u32>,
) -> Result<ListinvoicesResponse, Error> {
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let invoice_request = rpc
        .call(Request::ListInvoices(ListinvoicesRequest {
            label,
            invstring: None,
            payment_hash,
            offer_id: None,
            index,
            start,
            limit,
        }))
        .await
        .map_err(|e| anyhow!("Error calling listinvoices: {:?}", e))?;
    match invoice_request {
        Response::ListInvoices(info) => Ok(info),
        e => Err(anyhow!("Unexpected result in listinvoices: {:?}", e)),
    }
}
