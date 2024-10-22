use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::primitives::ShortChannelId;
use cln_rpc::ClnRpc;
use cln_rpc::{
    model::requests::*,
    model::responses::*,
    primitives::{Amount, PublicKey},
};

use log::debug;
use std::collections::BTreeMap;
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::grid::records::vec_records::Cell;
use tabled::grid::records::Records;
use tabled::settings::location::ByColumnName;
use tabled::settings::object::{Object, Rows};
use tabled::settings::{Alignment, Disable, Format, Modify, Panel, Width};

use tabled::Table;
use tokio::time::Instant;

use crate::structs::{
    Config, Forwards, ForwardsFilterStats, PagingIndex, PluginState, NO_ALIAS_SET,
};
use crate::util::{sort_columns, timestamp_to_localized_datetime_string, u64_to_sat_string};

pub async fn recent_forwards(
    rpc: &mut ClnRpc,
    peer_channels: &[ListpeerchannelsChannels],
    plugin: Plugin<PluginState>,
    config: &Config,
    now: Instant,
) -> Result<(Vec<Forwards>, ForwardsFilterStats), Error> {
    let now_utc = Utc::now().timestamp() as u64;
    let config_forwards_sec = config.forwards * 60 * 60;
    {
        if plugin.state().fw_index.lock().timestamp > now_utc - config_forwards_sec {
            *plugin.state().fw_index.lock() = PagingIndex::new();
            debug!("fw_index: forwards-age increased, resetting index");
        }
    }
    let mut fw_index = plugin.state().fw_index.lock().clone();
    debug!(
        "fw_index: start:{} timestamp:{}",
        fw_index.start, fw_index.timestamp
    );
    let forwards = rpc
        .call_typed(&ListforwardsRequest {
            status: Some(ListforwardsStatus::SETTLED),
            in_channel: None,
            out_channel: None,
            index: Some(ListforwardsIndex::CREATED),
            start: Some(fw_index.start),
            limit: None,
        })
        .await?
        .forwards;
    debug!(
        "List {} forwards. Total: {}ms",
        forwards.len(),
        now.elapsed().as_millis().to_string()
    );

    fw_index.timestamp = now_utc - config_forwards_sec;
    if let Some(last_fw) = forwards.last() {
        fw_index.start = last_fw.created_index.unwrap_or(u64::MAX);
    }

    let chanmap: BTreeMap<ShortChannelId, ListpeerchannelsChannels> = peer_channels
        .iter()
        .filter_map(|s| s.short_channel_id.map(|id| (id, s.clone())))
        .collect();

    let alias_map = plugin.state().alias_map.lock();

    let mut table = Vec::new();
    let mut filter_amt_sum_msat = 0;
    let mut filter_fee_sum_msat = 0;
    let mut filter_count = 0;

    for forward in forwards.into_iter() {
        if forward.received_time as u64 > now_utc - config_forwards_sec {
            let inchan = config
                .forwards_alias
                .then(|| {
                    chanmap.get(&forward.in_channel).and_then(|chan| {
                        alias_map
                            .get::<PublicKey>(&chan.peer_id)
                            .filter(|alias| alias.as_str() != (NO_ALIAS_SET))
                            .cloned()
                    })
                })
                .flatten()
                .unwrap_or_else(|| forward.in_channel.to_string());

            let fw_outchan = forward.out_channel.unwrap();
            let outchan = config
                .forwards_alias
                .then(|| {
                    chanmap.get(&fw_outchan).and_then(|chan| {
                        alias_map
                            .get::<PublicKey>(&chan.peer_id)
                            .filter(|alias| alias.as_str() != (NO_ALIAS_SET))
                            .cloned()
                    })
                })
                .flatten()
                .unwrap_or_else(|| fw_outchan.to_string());

            let mut should_filter = false;
            if forward.in_msat.msat() as i64 <= config.forwards_filter_amt_msat {
                should_filter = true;
            }
            if forward.fee_msat.unwrap().msat() as i64 <= config.forwards_filter_fee_msat {
                should_filter = true;
            }

            if should_filter {
                filter_amt_sum_msat += forward.in_msat.msat();
                filter_fee_sum_msat += forward.fee_msat.unwrap().msat();
                filter_count += 1;
            } else {
                table.push(Forwards {
                    received_time: (forward.received_time * 1_000.0) as u64,
                    received_time_str: timestamp_to_localized_datetime_string(
                        config,
                        forward.received_time as u64,
                    )?,
                    resolved_time: (forward.resolved_time.unwrap() * 1_000.0) as u64,
                    resolved_time_str: timestamp_to_localized_datetime_string(
                        config,
                        forward.resolved_time.unwrap() as u64,
                    )?,
                    in_channel_alias: if config.utf8 {
                        inchan
                    } else {
                        inchan.replace(|c: char| !c.is_ascii(), "?")
                    },
                    in_channel: forward.in_channel,
                    out_channel_alias: if config.utf8 {
                        outchan
                    } else {
                        outchan.replace(|c: char| !c.is_ascii(), "?")
                    },
                    out_channel: forward.out_channel.unwrap(),
                    in_msats: Amount::msat(&forward.in_msat),
                    out_msats: Amount::msat(&forward.out_msat.unwrap()),
                    fee_msats: Amount::msat(&forward.fee_msat.unwrap()),
                    in_sats: ((Amount::msat(&forward.in_msat) as f64) / 1_000.0).round() as u64,
                    out_sats: ((Amount::msat(&forward.out_msat.unwrap()) as f64) / 1_000.0).round()
                        as u64,
                    fee_sats: ((Amount::msat(&forward.fee_msat.unwrap()) as f64) / 1_000.0).round()
                        as u64,
                })
            }

            if let Some(c_index) = forward.created_index {
                if c_index < fw_index.start {
                    fw_index.start = c_index;
                }
            }
        }
    }
    if fw_index.start < u64::MAX {
        *plugin.state().fw_index.lock() = fw_index;
    }
    debug!(
        "Build forwards table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    table.sort_by_key(|x| x.resolved_time);
    Ok((
        table,
        ForwardsFilterStats {
            filter_amt_sum_msat,
            filter_fee_sum_msat,
            filter_count,
        },
    ))
}

pub fn format_forwards(
    table: Vec<Forwards>,
    config: &Config,
    filter_stats: ForwardsFilterStats,
) -> Result<String, Error> {
    let mut fwtable = Table::new(table);
    config.flow_style.apply(&mut fwtable);
    for head in Forwards::FIELD_NAMES_AS_ARRAY {
        if !config.forwards_columns.contains(&head.to_string()) {
            fwtable.with(Disable::column(ByColumnName::new(head)));
        }
    }
    let headers = fwtable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_string())
        .collect::<Vec<String>>();
    let records = fwtable.get_records_mut();
    if headers.len() != config.forwards_columns.len() {
        return Err(anyhow!(
            "Error formatting forwards! Length difference detected: {} {}",
            headers.join(","),
            config.forwards_columns.join(",")
        ));
    }
    sort_columns(records, &headers, &config.forwards_columns);

    if config.max_alias_length < 0 {
        fwtable.with(
            Modify::new(ByColumnName::new("in_channel")).with(
                Width::wrap(config.max_alias_length.unsigned_abs() as usize).keep_words(true),
            ),
        );
    } else {
        fwtable.with(
            Modify::new(ByColumnName::new("in_channel"))
                .with(Width::truncate(config.max_alias_length as usize).suffix("[..]")),
        );
    }

    if config.max_alias_length < 0 {
        fwtable.with(
            Modify::new(ByColumnName::new("out_channel")).with(
                Width::wrap(config.max_alias_length.unsigned_abs() as usize).keep_words(true),
            ),
        );
    } else {
        fwtable.with(
            Modify::new(ByColumnName::new("out_channel"))
                .with(Width::truncate(config.max_alias_length as usize).suffix("[..]")),
        );
    }

    fwtable.with(Modify::new(ByColumnName::new("in_sats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("in_sats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("in_msats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("in_msats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("out_sats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("out_sats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("out_msats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("out_msats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("fee_sats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("fee_sats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    fwtable.with(Modify::new(ByColumnName::new("fee_msats")).with(Alignment::right()));
    fwtable.with(
        Modify::new(ByColumnName::new("fee_msats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );

    fwtable.with(Panel::header("forwards"));
    fwtable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if filter_stats.filter_count > 0 {
        let filter_sum_result = format!(
            "\nFiltered {} forward{} with {} sats routed and {} msat fees.",
            filter_stats.filter_count,
            if filter_stats.filter_count == 1 {
                ""
            } else {
                "s"
            },
            u64_to_sat_string(
                config,
                ((filter_stats.filter_amt_sum_msat as f64) / 1_000.0).round() as u64
            )?,
            u64_to_sat_string(config, filter_stats.filter_fee_sum_msat)?,
        );
        fwtable.with(Panel::footer(filter_sum_result));
    }
    Ok(fwtable.to_string())
}
