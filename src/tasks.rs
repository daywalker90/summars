use std::{collections::BTreeMap, path::Path, time::Duration};

use anyhow::{anyhow, Error};
use cln_plugin::Plugin;
use cln_rpc::{
    model::requests::{ListnodesRequest, ListpeerchannelsRequest, ListpeersRequest},
    primitives::PublicKey,
    ClnRpc,
};
use log::{info, warn};
use serde_json::json;
use tokio::{
    fs::{self, File},
    time::{self, Instant},
};

use crate::{
    structs::{PeerAvailability, PluginState, NO_ALIAS_SET},
    util::{is_active_state, make_rpc_path},
};

pub async fn refresh_alias(plugin: Plugin<PluginState>) -> Result<(), Error> {
    let now = Instant::now();
    info!("Starting alias map refresh");
    plugin.state().alias_map.lock().clear();

    let rpc_path = make_rpc_path(&plugin);
    let mut rpc = ClnRpc::new(&rpc_path).await?;

    for peer in rpc
        .call_typed(&ListpeersRequest {
            id: None,
            level: None,
        })
        .await?
        .peers
    {
        let alias = rpc
            .call_typed(&ListnodesRequest { id: Some(peer.id) })
            .await?
            .nodes
            .first()
            .map(|node| node.alias.clone().unwrap_or(NO_ALIAS_SET.to_owned()));
        if let Some(a) = alias {
            plugin.state().alias_map.lock().insert(peer.id, a);
        }
    }

    info!("Alias map refresh done in: {}ms", now.elapsed().as_millis());
    Ok(())
}
pub async fn summars_refreshalias(
    p: Plugin<PluginState>,
    _v: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    match refresh_alias(p.clone()).await {
        Ok(()) => Ok(json!({"result":"success"})),
        Err(e) => Err(anyhow!("Error in refresh_alias thread: {}", e.to_string())),
    }
}

pub async fn trace_availability(plugin: Plugin<PluginState>) -> Result<(), Error> {
    let rpc_path = make_rpc_path(&plugin);
    let mut rpc = ClnRpc::new(&rpc_path).await?;

    let summarsdir = Path::new(&plugin.configuration().lightning_dir).join("summars");
    let availdbfile = summarsdir.join("availdb.json");
    let availdbfilecontent = fs::read_to_string(availdbfile.clone()).await;
    let mut persistpeers: BTreeMap<PublicKey, PeerAvailability>;

    match availdbfilecontent {
        Ok(file) => persistpeers = serde_json::from_str(&file).unwrap_or(BTreeMap::new()),
        Err(e) => {
            warn!("Could not open {}: {}. Maybe this is the first time using summars? Creating new file.", availdbfile.to_str().unwrap(),e);
            match fs::create_dir(summarsdir.clone()).await {
                Ok(_) => (),
                Err(e) => warn!("Warning: Could not create summars folder:{}", e),
            };
            File::create(availdbfile.clone()).await?;
            persistpeers = BTreeMap::new();
        }
    };

    let summary_availability_window: f64;
    let summary_availability_interval: f64;
    {
        let config = plugin.state().config.lock();
        summary_availability_window = config.availability_window as f64;
        summary_availability_interval = config.availability_interval as f64;
    }

    let avail_window = 60.0 * 60.0 * summary_availability_window;
    let mut editpeer;

    {
        *plugin.state().avail.lock() = persistpeers.clone();
    }

    loop {
        time::sleep(Duration::from_secs(summary_availability_interval as u64)).await;
        {
            let mut channels = rpc
                .call_typed(&ListpeerchannelsRequest { id: None })
                .await?
                .channels;
            channels.retain(is_active_state);
            for chan in channels {
                let leadwin = f64::max(
                    f64::min(
                        avail_window,
                        persistpeers
                            .get(&chan.peer_id)
                            .unwrap_or(&PeerAvailability {
                                count: 0,
                                connected: false,
                                avail: 0.0,
                            })
                            .count as f64
                            * summary_availability_interval,
                    ),
                    summary_availability_interval,
                );
                let samples = leadwin / summary_availability_interval;
                let alpha = 1.0 / samples;
                let beta = 1.0 - alpha;
                if let Some(data) = persistpeers.get_mut(&chan.peer_id) {
                    editpeer = data;
                } else {
                    persistpeers.insert(
                        chan.peer_id,
                        PeerAvailability {
                            count: 0,
                            connected: chan.peer_connected,
                            avail: if chan.peer_connected { 1.0 } else { 0.0 },
                        },
                    );
                    editpeer = persistpeers.get_mut(&chan.peer_id).unwrap();
                };
                if chan.peer_connected {
                    editpeer.connected = true;
                    editpeer.avail = 1.0 * alpha + editpeer.avail * beta;
                } else {
                    editpeer.connected = false;
                    editpeer.avail = 0.0 * alpha + editpeer.avail * beta
                }
                editpeer.count += 1;
            }
            *plugin.state().avail.lock() = persistpeers.clone();
            fs::write(availdbfile.clone(), serde_json::to_string(&persistpeers)?).await?;
            // debug!("{:?}", persistpeers);
        }
    }
}
