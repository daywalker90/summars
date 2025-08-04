extern crate serde_json;

use crate::config::get_startup_options;
use anyhow::anyhow;
use cln_plugin::{
    options::{BooleanConfigOption, ConfigOption, IntegerConfigOption, StringConfigOption},
    Builder,
};
use config::setconfig_callback;
use std::time::Duration;
use structs::PluginState;
use summary::summary;

use tasks::summars_refreshalias;
use tokio::{self, time};
mod config;
mod forwards;
mod invoices;
mod pays;
mod structs;
mod summary;
mod tasks;
mod util;

const OPT_COLUMNS: &str = "summars-columns";
const OPT_SORT_BY: &str = "summars-sort-by";
const OPT_EXCLUDE_CHANNEL_STATES: &str = "summars-exclude-states";
const OPT_FORWARDS: &str = "summars-forwards";
const OPT_FORWARDS_LIMIT: &str = "summars-forwards-limit";
const OPT_FORWARDS_COLUMNS: &str = "summars-forwards-columns";
const OPT_FORWARDS_FILTER_AMT: &str = "summars-forwards-filter-amount-msat";
const OPT_FORWARDS_FILTER_FEE: &str = "summars-forwards-filter-fee-msat";
const OPT_PAYS: &str = "summars-pays";
const OPT_PAYS_LIMIT: &str = "summars-pays-limit";
const OPT_PAYS_COLUMNS: &str = "summars-pays-columns";
const OPT_MAX_DESC_LENGTH: &str = "summars-max-description-length";
const OPT_INVOICES: &str = "summars-invoices";
const OPT_INVOICES_LIMIT: &str = "summars-invoices-limit";
const OPT_INVOICES_COLUMNS: &str = "summars-invoices-columns";
const OPT_MAX_LABEL_LENGTH: &str = "summars-max-label-length";
const OPT_INVOICES_FILTER_AMT: &str = "summars-invoices-filter-amount-msat";
const OPT_LOCALE: &str = "summars-locale";
const OPT_REFRESH_ALIAS: &str = "summars-refresh-alias";
const OPT_MAX_ALIAS_LENGTH: &str = "summars-max-alias-length";
const OPT_AVAILABILITY_INTERVAL: &str = "summars-availability-interval";
const OPT_AVAILABILITY_WINDOW: &str = "summars-availability-window";
const OPT_UTF8: &str = "summars-utf8";
const OPT_STYLE: &str = "summars-style";
const OPT_FLOW_STYLE: &str = "summars-flow-style";
const OPT_JSON: &str = "summars-json";

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    std::env::set_var("CLN_PLUGIN_LOG", "cln_plugin=info,cln_rpc=info,debug");
    let state = PluginState::new();
    let confplugin;
    let opt_columns: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_COLUMNS,
        "Enabled columns in the channel table. Available columns are: \
        `GRAPH_SATS,PERC_US,OUT_SATS,IN_SATS,TOTAL_SATS,SCID,MIN_HTLC,MAX_HTLC,FLAG,BASE,IN_BASE,\
        PPM,IN_PPM,ALIAS,PEER_ID,UPTIME,HTLCS,STATE` Default is `OUT_SATS,IN_SATS,SCID,MAX_HTLC,\
        FLAG,BASE,PPM,ALIAS,PEER_ID,UPTIME,HTLCS,STATE`",
    )
    .dynamic();
    let opt_sort_by: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_SORT_BY,
        "Sort by column name. Available values are: \
        `OUT_SATS,IN_SATS,SCID,MAX_HTLC,FLAG,BASE,PPM,ALIAS,PEER_ID,\
        UPTIME,HTLCS,STATE` Default is `SCID`",
    )
    .dynamic();
    let opt_exclude_channel_states: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_EXCLUDE_CHANNEL_STATES,
        "Exclude channels with given state from the summary table. Comma-separated string with \
        these available states: `OPENING,AWAIT_LOCK,OK,SHUTTING_DOWN,CLOSINGD_SIGEX,CLOSINGD_DONE,\
        AWAIT_UNILATERAL,FUNDING_SPEND,ONCHAIN,DUAL_OPEN,DUAL_COMITTED,DUAL_COMMIT_RDY,DUAL_AWAIT,\
        AWAIT_SPLICE` or `PUBLIC,PRIVATE` or `ONLINE,OFFLINE`",
    )
    .dynamic();
    let opt_forwards: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_FORWARDS,
        "Show last x hours of forwards. Default is `0`",
    )
    .dynamic();
    let opt_forwards_limit: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_FORWARDS_LIMIT,
        "Limit forwards table to the last x entries. Default is `0` (off)",
    )
    .dynamic();
    let opt_forwards_columns: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_FORWARDS_COLUMNS,
        "Enabled columns in the forwards table. Available columns are: \
        `received_time, resolved_time, in_channel, out_channel, in_alias, out_alias, \
        in_sats, in_msats, out_sats, out_msats, fee_sats, fee_msats, eff_fee_ppm` \
        Default is `resolved_time, in_alias, out_alias, in_sats, out_sats, fee_msats`",
    )
    .dynamic();
    let opt_forwards_filter_amt: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_FORWARDS_FILTER_AMT,
        "Filter forwards smaller than or equal to x msats. Default is `-1`",
    )
    .dynamic();
    let opt_forwards_filter_fee: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_FORWARDS_FILTER_FEE,
        "Filter forwards with less than or equal to x msats in fees. Default is `-1`",
    )
    .dynamic();
    let opt_pays: IntegerConfigOption =
        ConfigOption::new_i64_no_default(OPT_PAYS, "Show last x hours of pays. Default is `0`")
            .dynamic();
    let opt_pays_limit: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_PAYS_LIMIT,
        "Limit pays table to the last x entries. Default is `0` (off)",
    )
    .dynamic();
    let opt_pays_columns: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_PAYS_COLUMNS,
        "Enabled columns in the pays table. Available columns are: \
        `completed_at, payment_hash, sats_requested, msats_requested, sats_sent, msats_sent, \
        fee_sats, fee_msats, destination, description, preimage` \
        Default is `completed_at, payment_hash, sats_sent, fee_sats, destination`",
    )
    .dynamic();
    let opt_max_desc_length: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_MAX_DESC_LENGTH,
        "Max string length of an invoice description. Default is `30`",
    )
    .dynamic();
    let opt_invoices: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_INVOICES,
        "Show last x hours of invoices. Default is `0`",
    )
    .dynamic();
    let opt_invoices_limit: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_INVOICES_LIMIT,
        "Limit invoices table to the last x entries. Default is `0` (off)",
    )
    .dynamic();
    let opt_invoices_columns: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_INVOICES_COLUMNS,
        "Enabled columns in the invoices table. Available columns are: \
        `paid_at, label, description, sats_received, msats_received, payment_hash, preimage` \
        Default is `paid_at, label, sats_received, payment_hash`",
    )
    .dynamic();
    let opt_max_label_length: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_MAX_LABEL_LENGTH,
        "Max string length of an invoice label. Default is `30`",
    )
    .dynamic();
    let opt_invoices_filter_amt: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_INVOICES_FILTER_AMT,
        "Filter invoices smaller than or equal to x msats. Default is `-1`",
    )
    .dynamic();
    let opt_locale: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_LOCALE,
        "Set locale used for thousand delimiter etc.. Default is the system's \
        locale and as fallback `en-US` if none is found.",
    )
    .dynamic();
    let opt_refresh_alias: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_REFRESH_ALIAS,
        "Set frequency of alias cache refresh in hours. Default is `24`",
    )
    .dynamic();
    let opt_max_alias_length: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_MAX_ALIAS_LENGTH,
        "Max string length of alias. Default is `20`",
    )
    .dynamic();
    let opt_availability_interval: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_AVAILABILITY_INTERVAL,
        "How often in seconds the availability should be calculated. Default is `300`",
    )
    .dynamic();
    let opt_availability_window: IntegerConfigOption = ConfigOption::new_i64_no_default(
        OPT_AVAILABILITY_WINDOW,
        "How many hours the availability should be averaged over. Default is `72`",
    )
    .dynamic();
    let opt_utf8: BooleanConfigOption = ConfigOption::new_bool_no_default(
        OPT_UTF8,
        "Switch on/off special characters in node alias. Default is `true`",
    )
    .dynamic();
    let opt_style: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_STYLE,
        "Set style for the summary table. Default is `psql`",
    )
    .dynamic();
    let opt_flow_style: StringConfigOption = ConfigOption::new_str_no_default(
        OPT_FLOW_STYLE,
        "Set style for the flow tables (forwards, pays, invoices). Default is `blank`",
    )
    .dynamic();
    let opt_json: BooleanConfigOption =
        ConfigOption::new_bool_no_default(OPT_JSON, "Set output to json. Default is `false`")
            .dynamic();
    match Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(opt_columns)
        .option(opt_sort_by)
        .option(opt_exclude_channel_states)
        .option(opt_forwards)
        .option(opt_forwards_limit)
        .option(opt_forwards_columns)
        .option(opt_forwards_filter_amt)
        .option(opt_forwards_filter_fee)
        .option(opt_pays)
        .option(opt_pays_limit)
        .option(opt_pays_columns)
        .option(opt_max_desc_length)
        .option(opt_invoices)
        .option(opt_invoices_limit)
        .option(opt_invoices_columns)
        .option(opt_max_label_length)
        .option(opt_invoices_filter_amt)
        .option(opt_locale)
        .option(opt_refresh_alias)
        .option(opt_max_alias_length)
        .option(opt_availability_interval)
        .option(opt_availability_window)
        .option(opt_utf8)
        .option(opt_style)
        .option(opt_flow_style)
        .option(opt_json)
        .setconfig_callback(setconfig_callback)
        .rpcmethod(
            "summars",
            "Show summary of channels and optionally recent forwards",
            summary,
        )
        .rpcmethod(
            "summars-refreshalias",
            "Refresh the alias cache manually",
            summars_refreshalias,
        )
        .dynamic()
        .configure()
        .await?
    {
        Some(plugin) => {
            match get_startup_options(&plugin, state.clone()) {
                Ok(()) => &(),
                Err(e) => return plugin.disable(format!("{e}").as_str()).await,
            };
            log::info!("read startup options done");

            confplugin = plugin;
        }
        None => return Err(anyhow!("Error configuring the plugin!")),
    };
    if let Ok(plugin) = confplugin.start(state).await {
        log::info!("starting uptime task");
        let traceclone = plugin.clone();
        tokio::spawn(async move {
            match tasks::trace_availability(traceclone).await {
                Ok(()) => (),
                Err(e) => log::warn!("Error in trace_availability thread: {e}"),
            };
        });

        log::info!("starting refresh alias task");
        let aliasclone = plugin.clone();
        let alias_refresh_freq = plugin.state().config.lock().refresh_alias;
        tokio::spawn(async move {
            loop {
                match tasks::refresh_alias(aliasclone.clone()).await {
                    Ok(()) => (),
                    Err(e) => log::warn!("Error in refresh_alias thread: {e}"),
                };
                time::sleep(Duration::from_secs(alias_refresh_freq * 60 * 60)).await;
            }
        });
        plugin.join().await
    } else {
        Err(anyhow!("Error starting the plugin!"))
    }
}
