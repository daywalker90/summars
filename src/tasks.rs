use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    time::Duration,
};

use anyhow::{anyhow, Error};
use cln_plugin::Plugin;
use cln_rpc::{
    model::requests::{ListnodesRequest, ListpeerchannelsRequest},
    primitives::PublicKey,
    ClnRpc,
};
use serde_json::json;
use tokio::{
    fs::{self, File},
    time::{self, Instant},
};

use crate::{
    structs::{PeerAvailability, PluginState, NODE_GOSSIP_MISS, NO_ALIAS_SET},
    util::{is_active_state, make_rpc_path},
};

#[allow(clippy::cast_precision_loss)]
pub async fn refresh_alias(plugin: Plugin<PluginState>) -> Result<u64, Error> {
    let now = Instant::now();
    log::info!("Starting alias map refresh");
    plugin.state().alias_map.lock().clear();

    let rpc_path = make_rpc_path(&plugin);
    let mut rpc = ClnRpc::new(&rpc_path).await?;

    let listpeerchans = rpc
        .call_typed(&ListpeerchannelsRequest {
            id: None,
            short_channel_id: None,
        })
        .await?
        .channels;

    let peer_ids: HashSet<PublicKey> = listpeerchans.iter().map(|c| c.peer_id).collect();

    let peer_count = peer_ids.len();

    let mut miss_count: usize = 0;

    for peer_id in peer_ids {
        let node_response = rpc
            .call_typed(&ListnodesRequest { id: Some(peer_id) })
            .await?
            .nodes;
        let alias = if let Some(node) = node_response.first() {
            match &node.alias {
                Some(a) => a,
                None => NO_ALIAS_SET,
            }
        } else {
            miss_count += 1;
            NODE_GOSSIP_MISS
        };
        plugin
            .state()
            .alias_map
            .lock()
            .insert(peer_id, alias.to_owned());
    }

    let alias_refresh_freq = plugin.state().config.lock().refresh_alias;

    let miss_perc = (miss_count as f64 / peer_count as f64) * 100.0;

    let next_sleep = if miss_perc <= 5.0 {
        alias_refresh_freq * 60 * 60
    } else if miss_perc <= 10.0 {
        60 * 60
    } else if miss_perc <= 25.0 {
        10 * 60
    } else {
        60
    };

    log::info!(
        "Alias map refresh done in: {}ms. Next refresh in {}s",
        now.elapsed().as_millis(),
        next_sleep
    );
    Ok(next_sleep)
}

pub async fn summars_refreshalias(
    p: Plugin<PluginState>,
    _v: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    match refresh_alias(p.clone()).await {
        Ok(_s) => Ok(json!({"result":"success"})),
        Err(e) => Err(anyhow!("Error in refresh_alias thread: {e}")),
    }
}

#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
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
            log::warn!(
                "Could not open {}: {}. Maybe this is the first time using summars? \
            Creating new file.",
                availdbfile.to_str().unwrap(),
                e
            );
            match fs::create_dir(summarsdir.clone()).await {
                Ok(()) => (),
                Err(e) => log::warn!("Warning: Could not create summars folder:{e}"),
            }
            File::create(availdbfile.clone()).await?;
            persistpeers = BTreeMap::new();
        }
    }

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
                .call_typed(&ListpeerchannelsRequest {
                    id: None,
                    short_channel_id: None,
                })
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
                }
                if chan.peer_connected {
                    editpeer.connected = true;
                    editpeer.avail = 1.0 * alpha + editpeer.avail * beta;
                } else {
                    editpeer.connected = false;
                    editpeer.avail = 0.0 * alpha + editpeer.avail * beta;
                }
                editpeer.count += 1;
            }
            *plugin.state().avail.lock() = persistpeers.clone();
            fs::write(availdbfile.clone(), serde_json::to_string(&persistpeers)?).await?;
        }
    }
}
