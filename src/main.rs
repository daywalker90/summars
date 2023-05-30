extern crate serde_json;

use crate::config::{get_startup_options, read_config};
use anyhow::anyhow;
use cln_plugin::{options, Builder};
use log::{info, warn};
use std::time::Duration;
use structs::{Config, PluginState, PLUGIN_NAME};
use tables::summary;

use tasks::summars_refreshalias;
use tokio::{self, time};
mod config;
mod rpc;
mod structs;
mod tables;
mod tasks;
mod util;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    std::env::set_var("CLN_PLUGIN_LOG", "trace");
    let state = PluginState::new();
    let defaultconfig = Config::new();
    let confplugin;
    match Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(options::ConfigOption::new(
            &defaultconfig.show_pubkey.0,
            options::Value::OptBoolean,
            &format!(
                "Include pubkey in summary table. Default is {}",
                defaultconfig.show_pubkey.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.show_maxhtlc.0,
            options::Value::OptBoolean,
            &format!(
                "Include max_htlc in summary table. Default is {}",
                defaultconfig.show_maxhtlc.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.sort_by.0,
            options::Value::OptString,
            &format!(
                "Sort by column name. Default is {}",
                defaultconfig.sort_by.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.forwards.0,
            options::Value::OptInteger,
            &format!(
                "Show last x hours of forwards. Default is {}",
                defaultconfig.forwards.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.forward_alias.0,
            options::Value::OptBoolean,
            &format!(
                "Show peer alias for forward channels instead of scid's. Default is {}",
                defaultconfig.forward_alias.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.pays.0,
            options::Value::OptInteger,
            &format!(
                "Show last x hours of pays. Default is {}",
                defaultconfig.pays.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.invoices.0,
            options::Value::OptInteger,
            &format!(
                "Show last x hours of invoices. Default is {}",
                defaultconfig.invoices.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.locale.0,
            options::Value::OptString,
            &format!(
                "Set locale used for thousand delimiter etc.. Default is {:#?}",
                defaultconfig.locale.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.refresh_alias.0,
            options::Value::OptInteger,
            &format!(
                "Set frequency of alias cache refresh in hours. Default is {}",
                defaultconfig.refresh_alias.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.max_alias_length.0,
            options::Value::OptInteger,
            &format!(
                "Max string length of alias. Default is {}",
                defaultconfig.max_alias_length.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.availability_interval.0,
            options::Value::OptInteger,
            &format!(
                "How often in seconds the availability should be calculated. Default is {}",
                defaultconfig.availability_interval.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.availability_window.0,
            options::Value::OptInteger,
            &format!(
                "How many hours the availability should be averaged over. Default is {}",
                defaultconfig.availability_window.1
            ),
        ))
        .option(options::ConfigOption::new(
            &defaultconfig.utf8.0,
            options::Value::OptBoolean,
            &format!(
                "Switch on/off special characters in node alias. Default is {}",
                defaultconfig.utf8.1
            ),
        ))
        .rpcmethod(
            PLUGIN_NAME,
            "Show summary of channels and optionally recent forwards",
            summary,
        )
        .rpcmethod(
            &(PLUGIN_NAME.to_string() + "-refreshalias"),
            "Show summary of channels and optionally recent forwards",
            summars_refreshalias,
        )
        .dynamic()
        .configure()
        .await?
    {
        Some(plugin) => {
            match read_config(&plugin, state.clone()).await {
                Ok(()) => &(),
                Err(e) => return plugin.disable(format!("{}", e).as_str()).await,
            };
            info!("read config done");
            match get_startup_options(&plugin, state.clone()) {
                Ok(()) => &(),
                Err(e) => return plugin.disable(format!("{}", e).as_str()).await,
            };
            info!("read startup options done");

            confplugin = plugin;
        }
        None => return Err(anyhow!("Error configuring the plugin!")),
    };
    if let Ok(plugin) = confplugin.start(state).await {
        info!("starting uptime task");
        let traceclone = plugin.clone();
        tokio::spawn(async move {
            match tasks::trace_availability(traceclone).await {
                Ok(()) => (),
                Err(e) => warn!("Error in trace_availability thread: {}", e.to_string()),
            };
        });

        info!("starting refresh alias task");
        let aliasclone = plugin.clone();
        let alias_refresh_freq = plugin.state().config.lock().refresh_alias.1.clone();
        tokio::spawn(async move {
            loop {
                match tasks::refresh_alias(aliasclone.clone()).await {
                    Ok(()) => (),
                    Err(e) => warn!("Error in refresh_alias thread: {}", e.to_string()),
                };
                time::sleep(Duration::from_secs(alias_refresh_freq * 60 * 60)).await;
            }
        });
        plugin.join().await
    } else {
        Err(anyhow!("Error starting the plugin!"))
    }
}
