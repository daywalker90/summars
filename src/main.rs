extern crate serde_json;

use std::time::Duration;
#[cfg(feature = "hold")]
use std::{path::PathBuf, str::FromStr};

use anyhow::anyhow;
#[cfg(feature = "hold")]
use cln_plugin::Plugin;
use cln_plugin::{
    options::{
        ConfigOption,
        DefaultBooleanConfigOption,
        DefaultIntegerConfigOption,
        DefaultStringConfigOption,
        StringConfigOption,
    },
    Builder,
};
#[cfg(feature = "hold")]
use cln_rpc::ClnRpc;
use config::setconfig_callback;
#[cfg(feature = "hold")]
use serde_json::json;
use structs::PluginState;
use summary::summary;
use tasks::summars_refreshalias;
use tokio::{self, time};
#[cfg(feature = "hold")]
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint, Identity};

use crate::{
    config::get_startup_options,
    structs::{ForwardsColumns, InvoicesColumns, Opt, PaysColumns, SummaryColumns, TableColumn},
};
#[cfg(feature = "hold")]
use crate::{hold::hold_client::HoldClient, util::make_rpc_path};

mod config;
mod forwards;
mod invoices;
mod pays;
mod structs;
mod summary;
mod tasks;
mod util;

#[cfg(feature = "hold")]
pub mod hold {
    tonic::include_proto!("hold");
}

#[allow(clippy::too_many_lines)]
#[allow(clippy::cast_possible_wrap)]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), anyhow::Error> {
    std::env::set_var(
        "CLN_PLUGIN_LOG",
        "cln_plugin=info,cln_rpc=info,summars=trace,warn",
    );
    log_panics::init();
    let state = PluginState::new();
    let default_config = state.config.lock().clone();
    let confplugin;

    let default_columns = SummaryColumns::to_list_string(&default_config.columns);
    let opt_columns: DefaultStringConfigOption = ConfigOption::new_str_with_default(
        Opt::Columns.as_key(),
        &default_columns,
        "Enabled columns in the channel table. Available columns are: \
        `GRAPH_SATS,PERC_US,OUT_SATS,IN_SATS,TOTAL_SATS,SCID,MIN_HTLC,MAX_HTLC,FLAG,BASE,IN_BASE,\
        PPM,IN_PPM,ALIAS,PEER_ID,UPTIME,HTLCS,STATE`",
    )
    .dynamic();

    let default_sort_col = default_config.sort_by.to_string();
    let opt_sort_by: DefaultStringConfigOption = ConfigOption::new_str_with_default(
        Opt::SortBy.as_key(),
        &default_sort_col,
        "Sort by column name. Available values are: \
        `OUT_SATS,IN_SATS,SCID,MAX_HTLC,FLAG,BASE,PPM,ALIAS,PEER_ID,\
        UPTIME,HTLCS,STATE`",
    )
    .dynamic();

    let opt_exclude_channel_states: StringConfigOption = ConfigOption::new_str_no_default(
        Opt::ExcludeChannelStates.as_key(),
        "Exclude channels with given state from the summary table. Comma-separated string with \
        these available states: `OPENING,AWAIT_LOCK,OK,SHUTTING_DOWN,CLOSINGD_SIGEX,CLOSINGD_DONE,\
        AWAIT_UNILATERAL,FUNDING_SPEND,ONCHAIN,DUAL_OPEN,DUAL_COMITTED,DUAL_COMMIT_RDY,DUAL_AWAIT,\
        AWAIT_SPLICE` or `PUBLIC,PRIVATE` or `ONLINE,OFFLINE`",
    )
    .dynamic();

    let opt_forwards: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::Forwards.as_key(),
        default_config.forwards as i64,
        "Show last x hours of forwards.",
    )
    .dynamic();

    let opt_forwards_limit: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::ForwardsLimit.as_key(),
        default_config.forwards_limit as i64,
        "Limit forwards table to the last x entries.",
    )
    .dynamic();

    let default_forwards_columns =
        ForwardsColumns::to_list_string(&default_config.forwards_columns);
    let opt_forwards_columns: DefaultStringConfigOption = ConfigOption::new_str_with_default(
        Opt::ForwardsColumns.as_key(),
        &default_forwards_columns,
        "Enabled columns in the forwards table. Available columns are: \
        `received_time, resolved_time, in_channel, out_channel, in_alias, out_alias, \
        in_sats, in_msats, out_sats, out_msats, fee_sats, fee_msats, eff_fee_ppm`",
    )
    .dynamic();

    let opt_forwards_filter_amt: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::ForwardsFilterAmt.as_key(),
        default_config
            .forwards_filter_amt_msat
            .map_or(-1, |v| v as i64),
        "Filter forwards smaller than or equal to x msats.",
    )
    .dynamic();

    let opt_forwards_filter_fee: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::ForwardsFilterFee.as_key(),
        default_config
            .forwards_filter_fee_msat
            .map_or(-1, |v| v as i64),
        "Filter forwards with less than or equal to x msats in fees.",
    )
    .dynamic();

    let opt_pays: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::Pays.as_key(),
        default_config.pays as i64,
        "Show last x hours of pays.",
    )
    .dynamic();

    let opt_pays_limit: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::PaysLimit.as_key(),
        default_config.pays_limit as i64,
        "Limit pays table to the last x entries.",
    )
    .dynamic();

    let default_pays_columns = PaysColumns::to_list_string(&default_config.pays_columns);
    let opt_pays_columns: DefaultStringConfigOption = ConfigOption::new_str_with_default(
        Opt::PaysColumns.as_key(),
        &default_pays_columns,
        "Enabled columns in the pays table. Available columns are: \
        `completed_at, payment_hash, sats_requested, msats_requested, sats_sent, msats_sent, \
        fee_sats, fee_msats, destination, description, preimage`",
    )
    .dynamic();

    let opt_max_desc_length: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::MaxDescLength.as_key(),
        default_config.max_desc_length,
        "Max string length of an invoice description.",
    )
    .dynamic();

    let opt_invoices: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::Invoices.as_key(),
        default_config.invoices as i64,
        "Show last x hours of invoices.",
    )
    .dynamic();

    let opt_invoices_limit: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::InvoicesLimit.as_key(),
        default_config.invoices_limit as i64,
        "Limit invoices table to the last x entries.",
    )
    .dynamic();

    let default_invoices_columns =
        InvoicesColumns::to_list_string(&default_config.invoices_columns);
    let opt_invoices_columns: DefaultStringConfigOption = ConfigOption::new_str_with_default(
        Opt::InvoicesColumns.as_key(),
        &default_invoices_columns,
        "Enabled columns in the invoices table. Available columns are: \
        `paid_at, label, description, sats_received, msats_received, payment_hash, preimage`",
    )
    .dynamic();

    let opt_max_label_length: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::MaxLabelLength.as_key(),
        default_config.max_label_length,
        "Max string length of an invoice label.",
    )
    .dynamic();

    let opt_invoices_filter_amt: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::InvoicesFilterAmt.as_key(),
        default_config
            .invoices_filter_amt_msat
            .map_or(-1, |v| v as i64),
        "Filter invoices smaller than or equal to x msats.",
    )
    .dynamic();

    let default_locale = default_config.locale.to_string();
    let opt_locale: DefaultStringConfigOption = ConfigOption::new_str_with_default(
        Opt::Locale.as_key(),
        &default_locale,
        "Set locale used for thousand delimiter, time etc.",
    )
    .dynamic();

    let opt_refresh_alias: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::RefreshAlias.as_key(),
        default_config.refresh_alias as i64,
        "Set frequency of alias cache refresh in hours.",
    )
    .dynamic();

    let opt_max_alias_length: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::MaxAliasLength.as_key(),
        default_config.max_alias_length,
        "Max string length of alias.",
    )
    .dynamic();

    let opt_availability_interval: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::AvailabilityInterval.as_key(),
        default_config.availability_interval as i64,
        "How often in seconds the availability should be calculated.",
    )
    .dynamic();

    let opt_availability_window: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
        Opt::AvailabilityWindow.as_key(),
        default_config.availability_window as i64,
        "How many hours the availability should be averaged over.",
    )
    .dynamic();

    let opt_utf8: DefaultBooleanConfigOption = ConfigOption::new_bool_with_default(
        Opt::Utf8.as_key(),
        default_config.utf8,
        "Switch on/off special characters in node alias.",
    )
    .dynamic();

    let default_style = default_config.style.to_string();
    let opt_style: DefaultStringConfigOption = ConfigOption::new_str_with_default(
        Opt::Style.as_key(),
        &default_style,
        "Set style for the summary table.",
    )
    .dynamic();

    let default_flow_style = default_config.flow_style.to_string();
    let opt_flow_style: DefaultStringConfigOption = ConfigOption::new_str_with_default(
        Opt::FlowStyle.as_key(),
        &default_flow_style,
        "Set style for the flow tables (forwards, pays, invoices).",
    )
    .dynamic();

    let opt_json: DefaultBooleanConfigOption = ConfigOption::new_bool_with_default(
        Opt::Json.as_key(),
        default_config.json,
        "Set output to json.",
    )
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
            match get_startup_options(&plugin, &state) {
                Ok(()) => &(),
                Err(e) => return plugin.disable(format!("{e}").as_str()).await,
            };
            log::info!("read startup options done");

            confplugin = plugin;
        }
        None => return Err(anyhow!("Error configuring the plugin!")),
    }
    if let Ok(plugin) = confplugin.start(state).await {
        #[cfg(feature = "hold")]
        match check_hold_support(plugin.clone()).await {
            Ok(()) => {
                log::info!("Hold support activated");
            }
            Err(e) => log::info!("Hold support not activated: {e}"),
        }

        log::info!("starting uptime task");
        let plugin_clone_avail = plugin.clone();
        tokio::spawn(async move {
            match tasks::trace_availability(plugin_clone_avail).await {
                Ok(()) => (),
                Err(e) => log::warn!("Error in trace_availability thread: {e}"),
            }
        });

        log::info!("starting refresh alias task");
        let plugin_clone_alias = plugin.clone();
        tokio::spawn(async move {
            loop {
                let sleep_time = match tasks::refresh_alias(plugin_clone_alias.clone()).await {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("Error in refresh_alias thread: {e}");
                        60
                    }
                };
                time::sleep(Duration::from_secs(sleep_time)).await;
            }
        });
        plugin.join().await
    } else {
        Err(anyhow!("Error starting the plugin!"))
    }
}

#[cfg(feature = "hold")]
async fn check_hold_support(plugin: Plugin<PluginState>) -> Result<(), anyhow::Error> {
    let rpc_path = make_rpc_path(&plugin);
    let mut rpc = ClnRpc::new(&rpc_path).await?;
    let hold_grpc_host_response: serde_json::Value = rpc
        .call_raw("listconfigs", &json!({"config": "hold-grpc-host"}))
        .await?;

    let Some(hold_grpc_host_configs) = hold_grpc_host_response.get("configs") else {
        return Err(anyhow!("Unsopprted listconfigs response!"));
    };
    let Some(hold_grpc_host_config) = hold_grpc_host_configs.get("hold-grpc-host") else {
        return Err(anyhow!("hold-grpc-host config not found"));
    };
    let Some(hold_grpc_host_value) = hold_grpc_host_config.get("value_str") else {
        return Err(anyhow!("hold-grpc-host config not a string"));
    };
    let Some(hold_grpc_host) = hold_grpc_host_value.as_str() else {
        return Err(anyhow!("hold-grpc-host config not convertable to string"));
    };

    let hold_grpc_port_response: serde_json::Value = rpc
        .call_raw("listconfigs", &json!({"config": "hold-grpc-port"}))
        .await?;
    let Some(hold_grpc_port_configs) = hold_grpc_port_response.get("configs") else {
        return Err(anyhow!("Unsopprted listconfigs response!"));
    };
    let Some(hold_grpc_port_config) = hold_grpc_port_configs.get("hold-grpc-port") else {
        return Err(anyhow!("hold-grpc-port config not found"));
    };
    let Some(hold_grpc_port_value) = hold_grpc_port_config.get("value_int") else {
        return Err(anyhow!("hold-grpc-port config not a number"));
    };
    let hold_grpc_port = if let Some(hgh) = hold_grpc_port_value.as_u64() {
        u16::try_from(hgh)?
    } else {
        return Err(anyhow!("hold-grpc-port config not convertable to integer"));
    };

    let cert_dir = PathBuf::from_str(&plugin.configuration().lightning_dir)?.join("hold");

    log::debug!(
        "Searching {} for hold plugin certs",
        cert_dir.to_str().unwrap()
    );

    let mut retries = 10;

    let mut ca_cert;
    let mut client_cert;
    let client_key;

    loop {
        retries -= 1;
        if retries < 0 {
            return Err(anyhow!(
                "Could not find hold plugin certs in {:?}",
                cert_dir.to_str()
            ));
        }

        ca_cert = match tokio::fs::read(cert_dir.join("ca.pem")).await {
            Ok(o) => o,
            Err(_e) => {
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        client_cert = match tokio::fs::read(cert_dir.join("client.pem")).await {
            Ok(o) => o,
            Err(_e) => {
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        client_key = match tokio::fs::read(cert_dir.join("client-key.pem")).await {
            Ok(o) => o,
            Err(_e) => {
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        break;
    }

    let identity = Identity::from_pem(client_cert, client_key);

    let ca = Certificate::from_pem(ca_cert);

    let tls_config = ClientTlsConfig::new()
        .ca_certificate(ca)
        .identity(identity)
        .domain_name("hold");

    let hold_channel = Endpoint::from_shared(format!("https://{hold_grpc_host}:{hold_grpc_port}"))?
        .tls_config(tls_config)?
        .keep_alive_while_idle(true)
        .connect_lazy();
    *plugin.state().hold_client.lock() = Some(HoldClient::new(hold_channel));

    Ok(())
}
