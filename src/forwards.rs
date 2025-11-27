use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::{
    model::{
        requests::{
            ListforwardsIndex,
            ListforwardsRequest,
            ListforwardsStatus,
            WaitIndexname,
            WaitRequest,
            WaitSubsystem,
        },
        responses::{ListforwardsForwards, ListpeerchannelsChannels},
    },
    primitives::{Amount, PublicKey, ShortChannelId},
    ClnRpc,
};
use strum::IntoEnumIterator;
use tabled::{
    grid::records::{vec_records::Cell, Records},
    settings::{
        location::ByColumnName,
        object::{Object, Rows},
        Alignment,
        Format,
        Modify,
        Panel,
        Remove,
        Width,
    },
    Table,
};
use tokio::time::Instant;

use crate::{
    structs::{
        Config,
        Forwards,
        ForwardsColumns,
        FullNodeData,
        PagingIndex,
        PluginState,
        TableColumn,
        PAGE_SIZE,
    },
    util::{
        f64_to_u64_trunc,
        feeppm_effective_from_amts,
        get_alias_from_scid,
        rounded_div_u64,
        sort_columns,
        timestamp_to_localized_datetime_string,
        u64_to_sat_string,
    },
};

struct ForwardsAccumulator {
    oldest_updated: u64,
    fw_index: PagingIndex,
    forwards_map: BTreeMap<u64, Forwards>,
    filtered_set: HashSet<u64>,
}

pub async fn gather_forwards_data(
    rpc: &mut ClnRpc,
    peer_channels: &[ListpeerchannelsChannels],
    plugin: Plugin<PluginState>,
    config: &Config,
    now: Instant,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let now_utc = Utc::now().timestamp().unsigned_abs();
    let config_forwards_sec = config.forwards * 60 * 60;
    let cutoff_timestamp = now_utc - config_forwards_sec;

    fw_index_reset_if_needed(&plugin, config);

    let mut fw_index = *plugin.state().fw_index.lock();
    log::debug!(
        "1 fw_index: start:{} old_timestamp:{} new_timestamp:{}",
        fw_index.start,
        fw_index.timestamp,
        cutoff_timestamp
    );
    fw_index.timestamp = cutoff_timestamp;

    let chanmap: BTreeMap<ShortChannelId, ListpeerchannelsChannels> = peer_channels
        .iter()
        .filter_map(|s| s.short_channel_id.map(|id| (id, s.clone())))
        .collect();

    let oldest_updated = now_utc;

    let forwards_map: BTreeMap<u64, Forwards> = BTreeMap::new();

    let filtered_set: HashSet<u64> = HashSet::new();

    let mut forwards_acc = ForwardsAccumulator {
        oldest_updated,
        fw_index,
        forwards_map,
        filtered_set,
    };

    process_forward_batches(
        plugin.clone(),
        now,
        &mut forwards_acc,
        config,
        rpc,
        &chanmap,
        full_node_data,
    )
    .await?;

    post_process_forwards_data(&plugin, forwards_acc, config, full_node_data);

    Ok(())
}

async fn process_forward_batches(
    plugin: Plugin<PluginState>,
    now: Instant,
    forwards_acc: &mut ForwardsAccumulator,
    config: &Config,
    rpc: &mut ClnRpc,
    chanmap: &BTreeMap<ShortChannelId, ListpeerchannelsChannels>,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let current_index = rpc
        .call_typed(&WaitRequest {
            indexname: WaitIndexname::UPDATED,
            subsystem: WaitSubsystem::FORWARDS,
            nextvalue: 0,
        })
        .await?
        .updated
        .unwrap();
    log::debug!("Current forward index: {current_index}");

    let mut loop_count = 0;

    let (mut start_index, mut limit) = if forwards_acc.fw_index.start < u64::MAX {
        (forwards_acc.fw_index.start, None)
    } else {
        (
            current_index.saturating_sub(PAGE_SIZE - 1),
            Some(u32::try_from(PAGE_SIZE)?),
        )
    };

    while forwards_acc.oldest_updated >= forwards_acc.fw_index.timestamp {
        loop_count += 1;

        let settled_forwards = rpc
            .call_typed(&ListforwardsRequest {
                status: Some(ListforwardsStatus::SETTLED),
                in_channel: None,
                out_channel: None,
                index: Some(ListforwardsIndex::UPDATED),
                start: Some(start_index),
                limit,
            })
            .await?
            .forwards;

        let alias_map = plugin.state().alias_map.lock();

        build_forwards_table(
            forwards_acc,
            config,
            settled_forwards,
            chanmap,
            &alias_map,
            full_node_data,
        )?;

        if start_index == 0 {
            break;
        }
        limit = Some(u32::min(
            u32::try_from(PAGE_SIZE)?,
            u32::try_from(start_index)?,
        ));
        start_index = start_index.saturating_sub(PAGE_SIZE);
    }

    log::debug!(
        "Build forwards table in {loop_count} calls. Total: {}ms",
        now.elapsed().as_millis()
    );

    Ok(())
}

fn post_process_forwards_data(
    plugin: &Plugin<PluginState>,
    mut forwards_acc: ForwardsAccumulator,
    config: &Config,
    full_node_data: &mut FullNodeData,
) {
    log::debug!(
        "2 fw_index: start:{} timestamp:{}",
        forwards_acc.fw_index.start,
        forwards_acc.fw_index.timestamp
    );
    forwards_acc.fw_index.age = config.forwards;

    if config.forwards_limit > 0 && forwards_acc.forwards_map.len() > config.forwards_limit {
        full_node_data.forwards = forwards_acc
            .forwards_map
            .into_values()
            .rev()
            .take(config.forwards_limit)
            .rev()
            .collect();
    } else {
        full_node_data.forwards = forwards_acc.forwards_map.into_values().collect();
    }

    log::debug!(
        "3 fw_index: start:{} timestamp:{}",
        forwards_acc.fw_index.start,
        forwards_acc.fw_index.timestamp
    );
    *plugin.state().fw_index.lock() = forwards_acc.fw_index;

    full_node_data.forwards.sort_by_key(|x| x.resolved_time);
}

fn build_forwards_table(
    forwards_acc: &mut ForwardsAccumulator,
    config: &Config,
    settled_forwards: Vec<ListforwardsForwards>,
    chanmap: &BTreeMap<ShortChannelId, ListpeerchannelsChannels>,
    alias_map: &BTreeMap<PublicKey, String>,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    for forward in settled_forwards.into_iter().rev() {
        let Some(updated_index) = forward.updated_index else {
            continue;
        };

        let received_time = f64_to_u64_trunc(forward.received_time);
        if forwards_acc.forwards_map.contains_key(&updated_index) {
            continue;
        }
        if forwards_acc.filtered_set.contains(&updated_index) {
            continue;
        }
        if received_time <= forwards_acc.oldest_updated {
            forwards_acc.oldest_updated = received_time;
            if updated_index <= forwards_acc.fw_index.start {
                forwards_acc.fw_index.start = updated_index;
            }
        }
        if f64_to_u64_trunc(forward.resolved_time.unwrap_or(0.0)) > forwards_acc.fw_index.timestamp
        {
            let inchan = get_alias_from_scid(forward.in_channel, chanmap, alias_map);

            let fw_outchan = forward.out_channel.unwrap();
            let outchan = get_alias_from_scid(fw_outchan, chanmap, alias_map);

            let mut should_filter = false;
            if let Some(ff_msat) = config.forwards_filter_amt_msat {
                if forward.in_msat.msat() <= ff_msat {
                    should_filter = true;
                }
            }
            if let Some(ff_msat) = config.forwards_filter_fee_msat {
                if forward.fee_msat.unwrap().msat() <= ff_msat {
                    should_filter = true;
                }
            }

            if let Some(in_amt) = &mut full_node_data.totals.forwards_amount_in_msat {
                *in_amt += forward.in_msat.msat();
            } else {
                full_node_data.totals.forwards_amount_in_msat = Some(forward.in_msat.msat());
            }
            if let Some(out_amt) = &mut full_node_data.totals.forwards_amount_out_msat {
                *out_amt += forward.out_msat.unwrap().msat();
            } else {
                full_node_data.totals.forwards_amount_out_msat =
                    Some(forward.out_msat.unwrap().msat());
            }
            if let Some(fee_amt) = &mut full_node_data.totals.forwards_fees_msat {
                *fee_amt += forward.fee_msat.unwrap().msat();
            } else {
                full_node_data.totals.forwards_fees_msat = Some(forward.fee_msat.unwrap().msat());
            }

            let result = Forwards {
                received_time: received_time * 1_000,
                received_time_str: timestamp_to_localized_datetime_string(
                    config,
                    f64_to_u64_trunc(forward.received_time),
                )?,
                resolved_time: f64_to_u64_trunc(forward.resolved_time.unwrap()) * 1_000,
                resolved_time_str: timestamp_to_localized_datetime_string(
                    config,
                    f64_to_u64_trunc(forward.resolved_time.unwrap()),
                )?,
                in_alias: if config.utf8 {
                    inchan
                } else {
                    inchan.replace(|c: char| !c.is_ascii(), "?")
                },
                in_channel: forward.in_channel,
                out_alias: if config.utf8 {
                    outchan
                } else {
                    outchan.replace(|c: char| !c.is_ascii(), "?")
                },
                out_channel: forward.out_channel.unwrap(),
                in_msats: Amount::msat(&forward.in_msat),
                out_msats: Amount::msat(&forward.out_msat.unwrap()),
                fee_msats: Amount::msat(&forward.fee_msat.unwrap()),
                in_sats: rounded_div_u64(forward.in_msat.msat(), 1000),
                out_sats: rounded_div_u64(forward.out_msat.unwrap().msat(), 1_000),
                fee_sats: rounded_div_u64(forward.fee_msat.unwrap().msat(), 1_000),
                eff_fee_ppm: feeppm_effective_from_amts(
                    forward.in_msat.msat(),
                    forward.out_msat.unwrap().msat(),
                ),
            };

            if should_filter {
                full_node_data.forwards_filter_stats.amt_sum_msat += result.in_msats;
                full_node_data.forwards_filter_stats.fee_sum_msat += result.fee_msats;
                full_node_data.forwards_filter_stats.count += 1;
                forwards_acc.filtered_set.insert(updated_index);
            } else {
                forwards_acc.forwards_map.insert(updated_index, result);
            }
        }
    }
    Ok(())
}

fn fw_index_reset_if_needed(plugin: &Plugin<PluginState>, config: &Config) {
    let mut fw_index = plugin.state().fw_index.lock();

    if fw_index.age != config.forwards {
        *fw_index = PagingIndex::new();
        log::debug!("fw_index: forwards window changed, resetting index");
    }
}

#[allow(clippy::too_many_lines)]
pub fn format_forwards(
    config: &Config,
    full_node_data: &mut FullNodeData,
) -> Result<String, Error> {
    let count = full_node_data.forwards.len();
    let mut fwtable = Table::new(&full_node_data.forwards);
    config.flow_style.apply(&mut fwtable);
    for head in ForwardsColumns::iter() {
        if !config.forwards_columns.contains(&head) {
            fwtable.with(Remove::column(ByColumnName::new(head.to_string())));
        }
    }
    let headers = fwtable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| ForwardsColumns::parse_column(s.text()).unwrap())
        .collect::<Vec<ForwardsColumns>>();
    let records = fwtable.get_records_mut();
    if headers.len() != config.forwards_columns.len() {
        return Err(anyhow!(
            "Error formatting forwards! Length difference detected: {} {}",
            ForwardsColumns::to_list_string(&headers),
            ForwardsColumns::to_list_string(&config.forwards_columns)
        ));
    }
    sort_columns(records, &headers, &config.forwards_columns);

    for numerical in ForwardsColumns::NUMERICAL {
        fwtable
            .with(Modify::new(ByColumnName::new(numerical.to_string())).with(Alignment::right()));
        fwtable.with(
            Modify::new(ByColumnName::new(numerical.to_string()).not(Rows::first())).with(
                Format::content(|s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()),
            ),
        );
    }

    if config.max_alias_length < 0 {
        fwtable.with(
            Modify::new(ByColumnName::new(ForwardsColumns::in_alias.to_string())).with(
                Width::wrap(usize::try_from(config.max_alias_length.unsigned_abs())?)
                    .keep_words(true),
            ),
        );
    } else {
        fwtable.with(
            Modify::new(ByColumnName::new(ForwardsColumns::in_alias.to_string()))
                .with(Width::truncate(usize::try_from(config.max_alias_length)?).suffix("[..]")),
        );
    }

    if config.max_alias_length < 0 {
        fwtable.with(
            Modify::new(ByColumnName::new(ForwardsColumns::out_alias.to_string())).with(
                Width::wrap(usize::try_from(config.max_alias_length.unsigned_abs())?)
                    .keep_words(true),
            ),
        );
    } else {
        fwtable.with(
            Modify::new(ByColumnName::new(ForwardsColumns::out_alias.to_string()))
                .with(Width::truncate(usize::try_from(config.max_alias_length)?).suffix("[..]")),
        );
    }

    fwtable.with(Panel::header(format!(
        "forwards (last {}h, limit: {})",
        config.forwards,
        if config.forwards_limit > 0 {
            format!("{}/{}", count, config.forwards_limit)
        } else {
            "off".to_owned()
        }
    )));
    fwtable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if full_node_data.forwards_filter_stats.count > 0 {
        let filter_sum_result = format!(
            "\nFiltered {} forward{} with {} sats routed and {} msat fees.",
            full_node_data.forwards_filter_stats.count,
            if full_node_data.forwards_filter_stats.count == 1 {
                ""
            } else {
                "s"
            },
            u64_to_sat_string(
                config,
                rounded_div_u64(full_node_data.forwards_filter_stats.amt_sum_msat, 1_000)
            )?,
            u64_to_sat_string(config, full_node_data.forwards_filter_stats.fee_sum_msat)?,
        );
        fwtable.with(Panel::footer(filter_sum_result));
    }
    if full_node_data.totals.forwards_amount_in_msat.is_some() {
        let forwards_totals = format!(
            "\nTotal forwards stats in the last {}h: {} in_sats {} out_sats {} fee_sats",
            config.forwards,
            u64_to_sat_string(
                config,
                rounded_div_u64(full_node_data.totals.forwards_amount_in_msat.unwrap(), 1000)
            )?,
            u64_to_sat_string(
                config,
                rounded_div_u64(
                    full_node_data.totals.forwards_amount_out_msat.unwrap(),
                    1000
                )
            )?,
            u64_to_sat_string(
                config,
                rounded_div_u64(full_node_data.totals.forwards_fees_msat.unwrap(), 1000)
            )?
        );
        fwtable.with(Panel::footer(forwards_totals));
    }

    Ok(fwtable.to_string())
}
