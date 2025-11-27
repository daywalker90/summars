use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::{
    model::{
        requests::{
            ListinvoicesIndex,
            ListinvoicesRequest,
            WaitIndexname,
            WaitRequest,
            WaitSubsystem,
        },
        responses::{ListinvoicesInvoices, ListinvoicesInvoicesStatus},
    },
    primitives::Amount,
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
        FullNodeData,
        Invoices,
        InvoicesColumns,
        PagingIndex,
        PluginState,
        TableColumn,
        PAGE_SIZE,
    },
    util::{
        hex_encode,
        replace_escaping_chars,
        rounded_div_u64,
        sort_columns,
        timestamp_to_localized_datetime_string,
        u64_to_sat_string,
    },
};

struct InvoicesAccumulator {
    oldest_updated: u64,
    inv_index: PagingIndex,
    invoices_map: BTreeMap<u64, Invoices>,
    filtered_set: HashSet<u64>,
}

pub async fn gather_invoices_data(
    plugin: Plugin<PluginState>,
    rpc: &mut ClnRpc,
    config: &Config,
    now: Instant,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let now_utc = Utc::now().timestamp().unsigned_abs();
    let config_invoices_sec = config.invoices * 60 * 60;
    let cutoff_timestamp = now_utc - config_invoices_sec;

    inv_index_reset_if_needed(&plugin, config);

    let mut inv_index = *plugin.state().inv_index.lock();
    log::debug!(
        "1 inv_index: start:{} old_timestamp:{} new_timestamp:{}",
        inv_index.start,
        inv_index.timestamp,
        cutoff_timestamp
    );
    inv_index.timestamp = cutoff_timestamp;

    let oldest_updated = now_utc;

    let invoices_map: BTreeMap<u64, Invoices> = BTreeMap::new();

    let filtered_set: HashSet<u64> = HashSet::new();

    let mut invoices_acc = InvoicesAccumulator {
        oldest_updated,
        inv_index,
        invoices_map,
        filtered_set,
    };

    process_invoice_batches(now, &mut invoices_acc, config, rpc, full_node_data).await?;

    post_process_invoices_data(&plugin, invoices_acc, config, full_node_data);

    Ok(())
}

async fn process_invoice_batches(
    now: Instant,
    invoices_acc: &mut InvoicesAccumulator,
    config: &Config,
    rpc: &mut ClnRpc,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let current_index = rpc
        .call_typed(&WaitRequest {
            indexname: WaitIndexname::UPDATED,
            subsystem: WaitSubsystem::INVOICES,
            nextvalue: 0,
        })
        .await?
        .updated
        .unwrap();
    log::debug!("Current invoices index: {current_index}");

    let mut loop_count = 0;

    let (mut start_index, mut limit) = if invoices_acc.inv_index.start < u64::MAX {
        (invoices_acc.inv_index.start, None)
    } else {
        (
            current_index.saturating_sub(PAGE_SIZE - 1),
            Some(u32::try_from(PAGE_SIZE)?),
        )
    };

    while invoices_acc.oldest_updated >= invoices_acc.inv_index.timestamp {
        loop_count += 1;

        let invoices = rpc
            .call_typed(&ListinvoicesRequest {
                label: None,
                invstring: None,
                payment_hash: None,
                offer_id: None,
                index: Some(ListinvoicesIndex::UPDATED),
                start: Some(start_index),
                limit,
            })
            .await?
            .invoices;

        build_invoices_table(invoices_acc, invoices, full_node_data, config)?;

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
        "Build invoices table in {loop_count} calls. Total: {}ms",
        now.elapsed().as_millis()
    );

    Ok(())
}

fn post_process_invoices_data(
    plugin: &Plugin<PluginState>,
    mut invoices_acc: InvoicesAccumulator,
    config: &Config,
    full_node_data: &mut FullNodeData,
) {
    log::debug!(
        "2 inv_index: start:{} timestamp:{}",
        invoices_acc.inv_index.start,
        invoices_acc.inv_index.timestamp
    );
    invoices_acc.inv_index.age = config.invoices;

    if config.invoices_limit > 0 && invoices_acc.invoices_map.len() > config.invoices_limit {
        full_node_data.invoices = invoices_acc
            .invoices_map
            .into_values()
            .rev()
            .take(config.invoices_limit)
            .rev()
            .collect();
    } else {
        full_node_data.invoices = invoices_acc.invoices_map.into_values().collect();
    }

    log::debug!(
        "3 inv_index: start:{} timestamp:{}",
        invoices_acc.inv_index.start,
        invoices_acc.inv_index.timestamp
    );
    *plugin.state().inv_index.lock() = invoices_acc.inv_index;

    full_node_data.invoices.sort_by_key(|x| x.paid_at);
}

fn build_invoices_table(
    invoices_acc: &mut InvoicesAccumulator,
    invoices: Vec<ListinvoicesInvoices>,
    full_node_data: &mut FullNodeData,
    config: &Config,
) -> Result<(), Error> {
    for invoice in invoices.into_iter().rev() {
        if ListinvoicesInvoicesStatus::PAID == invoice.status {
            let Some(updated_index) = invoice.updated_index else {
                continue;
            };
            let Some(inv_paid_at) = invoice.paid_at else {
                continue;
            };
            if invoices_acc.invoices_map.contains_key(&updated_index) {
                continue;
            }
            if invoices_acc.filtered_set.contains(&updated_index) {
                continue;
            }
            if inv_paid_at <= invoices_acc.oldest_updated {
                invoices_acc.oldest_updated = inv_paid_at;
                if updated_index <= invoices_acc.inv_index.start {
                    invoices_acc.inv_index.start = updated_index;
                }
            }
            if inv_paid_at >= invoices_acc.inv_index.timestamp {
                if let Some(inv_amt) = &mut full_node_data.totals.invoices_amount_received_msat {
                    *inv_amt += invoice.amount_received_msat.unwrap().msat();
                } else {
                    full_node_data.totals.invoices_amount_received_msat =
                        Some(invoice.amount_received_msat.unwrap().msat());
                }

                let result = Invoices {
                    paid_at: invoice.paid_at.unwrap(),
                    paid_at_str: timestamp_to_localized_datetime_string(
                        config,
                        invoice.paid_at.unwrap(),
                    )?,
                    label: invoice.label,
                    msats_received: Amount::msat(&invoice.amount_received_msat.unwrap()),
                    sats_received: rounded_div_u64(
                        invoice.amount_received_msat.unwrap().msat(),
                        1_000,
                    ),
                    description: invoice.description.unwrap_or_default(),
                    payment_hash: invoice.payment_hash.to_string(),
                    preimage: hex_encode(&invoice.payment_preimage.unwrap().to_vec()),
                };

                if let Some(if_amt) = config.invoices_filter_amt_msat {
                    if result.msats_received <= if_amt {
                        full_node_data.invoices_filter_stats.filter_count += 1;
                        full_node_data.invoices_filter_stats.filter_amt_sum_msat +=
                            result.msats_received;
                        invoices_acc.filtered_set.insert(updated_index);

                        continue;
                    }
                }

                invoices_acc.invoices_map.insert(updated_index, result);
            }
        }
    }
    Ok(())
}

fn inv_index_reset_if_needed(plugin: &Plugin<PluginState>, config: &Config) {
    let mut inv_index = plugin.state().inv_index.lock();

    if inv_index.age != config.invoices {
        *inv_index = PagingIndex::new();
        log::debug!("inv_index: invoices window changed, resetting index");
    }
}

#[allow(clippy::too_many_lines)]
pub fn format_invoices(
    config: &Config,
    full_node_data: &mut FullNodeData,
) -> Result<String, Error> {
    let count = full_node_data.invoices.len();
    let mut invoicestable = Table::new(&full_node_data.invoices);
    config.flow_style.apply(&mut invoicestable);
    for head in InvoicesColumns::iter() {
        if !config.invoices_columns.contains(&head) {
            invoicestable.with(Remove::column(ByColumnName::new(head.to_string())));
        }
    }
    let headers = invoicestable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| InvoicesColumns::parse_column(s.text()).unwrap())
        .collect::<Vec<InvoicesColumns>>();
    let records = invoicestable.get_records_mut();
    if headers.len() != config.invoices_columns.len() {
        return Err(anyhow!(
            "Error formatting invoices! Length difference detected: {} {}",
            InvoicesColumns::to_list_string(&headers),
            InvoicesColumns::to_list_string(&config.invoices_columns)
        ));
    }
    sort_columns(records, &headers, &config.invoices_columns);

    for numerical in InvoicesColumns::NUMERICAL {
        invoicestable
            .with(Modify::new(ByColumnName::new(numerical.to_string())).with(Alignment::right()));
        invoicestable.with(
            Modify::new(ByColumnName::new(numerical.to_string()).not(Rows::first())).with(
                Format::content(|s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()),
            ),
        );
    }

    if config.max_desc_length < 0 {
        invoicestable.with(
            Modify::new(ByColumnName::new(InvoicesColumns::description.to_string()))
                .with(Format::content(replace_escaping_chars))
                .with(
                    Width::wrap(usize::try_from(config.max_desc_length.unsigned_abs())?)
                        .keep_words(true),
                ),
        );
    } else {
        invoicestable.with(
            Modify::new(ByColumnName::new(InvoicesColumns::description.to_string()))
                .with(Format::content(replace_escaping_chars))
                .with(Width::truncate(usize::try_from(config.max_desc_length)?).suffix("[..]")),
        );
    }

    if config.max_label_length < 0 {
        invoicestable.with(
            Modify::new(ByColumnName::new(InvoicesColumns::label.to_string())).with(
                Width::wrap(usize::try_from(config.max_label_length.unsigned_abs())?)
                    .keep_words(true),
            ),
        );
    } else {
        invoicestable.with(
            Modify::new(ByColumnName::new(InvoicesColumns::label.to_string()))
                .with(Width::truncate(usize::try_from(config.max_label_length)?).suffix("[..]")),
        );
    }

    invoicestable.with(Panel::header(format!(
        "invoices (last {}h, limit: {})",
        config.invoices,
        if config.invoices_limit > 0 {
            format!("{}/{}", count, config.invoices_limit)
        } else {
            "off".to_owned()
        }
    )));
    invoicestable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if full_node_data.invoices_filter_stats.filter_count > 0 {
        let filter_sum_result = format!(
            "\nFiltered {} invoice{} with {} sats total.",
            full_node_data.invoices_filter_stats.filter_count,
            if full_node_data.invoices_filter_stats.filter_count == 1 {
                ""
            } else {
                "s"
            },
            u64_to_sat_string(
                config,
                rounded_div_u64(
                    full_node_data.invoices_filter_stats.filter_amt_sum_msat,
                    1_000
                )
            )?
        );
        invoicestable.with(Panel::footer(filter_sum_result));
    }

    if let Some(inv_total) = full_node_data.totals.invoices_amount_received_msat {
        let invoices_total = format!(
            "\nTotal invoices stats in the last {}h: {} sats_received",
            config.invoices,
            u64_to_sat_string(config, rounded_div_u64(inv_total, 1000))?,
        );
        invoicestable.with(Panel::footer(invoices_total));
    }

    Ok(invoicestable.to_string())
}
