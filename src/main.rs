extern crate serde_json;

use crate::config::{get_startup_options, read_config};
use anyhow::anyhow;
use cln_plugin::{
    options::{BooleanConfigOption, ConfigOption, IntegerConfigOption, StringConfigOption},
    Builder,
};
use log::{info, warn};
use std::time::Duration;
use structs::PluginState;
use tables::summary;

use tasks::summars_refreshalias;
use tokio::{self, time};
mod config;
mod rpc;
mod structs;
mod tables;
mod tasks;
mod util;

const OPT_COLUMNS: StringConfigOption = ConfigOption::new_str_no_default(
    "summars-columns",
    "Enabled columns in the channel table. Allowed columns are: \
    `GRAPH_SATS,OUT_SATS,IN_SATS,SCID,MAX_HTLC,FLAG,BASE,PPM,ALIAS,PEER_ID,UPTIME,HTLCS,STATE` \
    Default is `OUT_SATS,IN_SATS,SCID,MAX_HTLC,FLAG,BASE,PPM,ALIAS,PEER_ID,UPTIME,HTLCS,STATE`",
);
const OPT_SORT_BY: StringConfigOption =
    ConfigOption::new_str_no_default("summars-sort-by", "Sort by column name. Default is `SCID`");
const OPT_FORWARDS: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-forwards",
    "Show last x hours of forwards. Default is `0`",
);
const OPT_FORWARDS_FILTER_AMT: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-forwards-filter-amount-msat",
    "Filter forwards smaller than or equal to x msats. Default is `-1`",
);
const OPT_FORWARDS_FILTER_FEE: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-forwards-filter-fee-msat",
    "Filter forwards with less than or equal to x msats in fees. Default is `-1`",
);
const OPT_FORWARDS_ALIAS: BooleanConfigOption = ConfigOption::new_bool_no_default(
    "summars-forwards-alias",
    "Show peer alias for forward channels instead of scid's. Default is `true`",
);
const OPT_PAYS: IntegerConfigOption =
    ConfigOption::new_i64_no_default("summars-pays", "Show last x hours of pays. Default is `0`");
const OPT_INVOICES: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-invoices",
    "Show last x hours of invoices. Default is `0`",
);
const OPT_INVOICES_FILTER_AMT: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-invoices-filter-amount-msat",
    "Filter invoices smaller than or equal to x msats. Default is `-1`",
);
const OPT_LOCALE: StringConfigOption = ConfigOption::new_str_no_default(
    "summars-locale",
    "Set locale used for thousand delimiter etc.. Default is the system's \
        locale and as fallback `en-US` if none is found.",
);
const OPT_REFRESH_ALIAS: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-refresh-alias",
    "Set frequency of alias cache refresh in hours. Default is `24`",
);
const OPT_MAX_ALIAS_LENGTH: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-max-alias-length",
    "Max string length of alias. Default is `20`",
);
const OPT_AVAILABILITY_INTERVAL: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-availability-interval",
    "How often in seconds the availability should be calculated. Default is `300`",
);
const OPT_AVAILABILITY_WINDOW: IntegerConfigOption = ConfigOption::new_i64_no_default(
    "summars-availability-window",
    "How many hours the availability should be averaged over. Default is `72`",
);
const OPT_UTF8: BooleanConfigOption = ConfigOption::new_bool_no_default(
    "summars-utf8",
    "Switch on/off special characters in node alias. Default is `true`",
);
const OPT_STYLE: StringConfigOption = ConfigOption::new_str_no_default(
    "summars-style",
    "Set style for the summary table. Default is `psql`",
);
const OPT_FLOW_STYLE: StringConfigOption = ConfigOption::new_str_no_default(
    "summars-flow-style",
    "Set style for the flow tables (forwards, pays, invoices). Default is `blank`",
);

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    std::env::set_var("CLN_PLUGIN_LOG", "cln_plugin=info,cln_rpc=info,debug");
    let state = PluginState::new();
    let confplugin;
    match Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(OPT_COLUMNS)
        .option(OPT_SORT_BY)
        .option(OPT_FORWARDS)
        .option(OPT_FORWARDS_FILTER_AMT)
        .option(OPT_FORWARDS_FILTER_FEE)
        .option(OPT_FORWARDS_ALIAS)
        .option(OPT_PAYS)
        .option(OPT_INVOICES)
        .option(OPT_INVOICES_FILTER_AMT)
        .option(OPT_LOCALE)
        .option(OPT_REFRESH_ALIAS)
        .option(OPT_MAX_ALIAS_LENGTH)
        .option(OPT_AVAILABILITY_INTERVAL)
        .option(OPT_AVAILABILITY_WINDOW)
        .option(OPT_UTF8)
        .option(OPT_STYLE)
        .option(OPT_FLOW_STYLE)
        .rpcmethod(
            "summars",
            "Show summary of channels and optionally recent forwards",
            summary,
        )
        .rpcmethod(
            "summars-refreshalias",
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
        let alias_refresh_freq = plugin.state().config.lock().refresh_alias.value;
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
