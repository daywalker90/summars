use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, Error};
use chrono::Utc;
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
    structs::{Config, FullNodeData, Invoices, InvoicesColumns, TableColumn, PAGE_SIZE},
    util::{
        accumulate_msat,
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
    cutoff_timestamp: u64,
    invoices_map: BTreeMap<u64, Invoices>,
    filtered_set: HashSet<u64>,
}

pub async fn gather_invoices_data(
    rpc: &mut ClnRpc,
    config: &Config,
    now: Instant,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let now_utc = Utc::now().timestamp().unsigned_abs();
    let config_invoices_sec = config.invoices * 60 * 60;
    let cutoff_timestamp = now_utc - config_invoices_sec;

    let oldest_updated = now_utc;

    let invoices_map: BTreeMap<u64, Invoices> = BTreeMap::new();

    let filtered_set: HashSet<u64> = HashSet::new();

    let mut invoices_acc = InvoicesAccumulator {
        oldest_updated,
        cutoff_timestamp,
        invoices_map,
        filtered_set,
    };

    process_invoice_batches(now, &mut invoices_acc, config, rpc, full_node_data).await?;

    limit_and_sort_invoices_data(invoices_acc, config, full_node_data);

    Ok(())
}

async fn process_invoice_batches(
    now: Instant,
    invoices_acc: &mut InvoicesAccumulator,
    config: &Config,
    rpc: &mut ClnRpc,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let mut current_index = rpc
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

    current_index = current_index.saturating_sub(PAGE_SIZE - 1);
    let mut limit = u32::try_from(PAGE_SIZE)?;

    while invoices_acc.oldest_updated >= invoices_acc.cutoff_timestamp {
        loop_count += 1;

        let invoices = rpc
            .call_typed(&ListinvoicesRequest {
                label: None,
                invstring: None,
                payment_hash: None,
                offer_id: None,
                index: Some(ListinvoicesIndex::UPDATED),
                start: Some(current_index),
                limit: Some(limit),
            })
            .await?
            .invoices;

        build_invoices_table(invoices_acc, invoices, full_node_data, config)?;

        if current_index <= 1 {
            break;
        }
        limit = u32::min(u32::try_from(PAGE_SIZE)?, u32::try_from(current_index)?);
        current_index = current_index.saturating_sub(PAGE_SIZE);
    }

    log::debug!(
        "Build invoices table in {loop_count} calls. Total: {}ms",
        now.elapsed().as_millis()
    );

    Ok(())
}

fn limit_and_sort_invoices_data(
    invoices_acc: InvoicesAccumulator,
    config: &Config,
    full_node_data: &mut FullNodeData,
) {
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
            }
            if inv_paid_at >= invoices_acc.cutoff_timestamp {
                accumulate_msat(
                    &mut full_node_data.totals.invoices_amount_received_msat,
                    invoice.amount_received_msat.unwrap().msat(),
                );

                let msats_received = Amount::msat(&invoice.amount_received_msat.unwrap());

                if let Some(if_amt) = config.invoices_filter_amt_msat {
                    if msats_received <= if_amt {
                        full_node_data.invoices_filter_stats.filter_count += 1;
                        full_node_data.invoices_filter_stats.filter_amt_sum_msat += msats_received;
                        invoices_acc.filtered_set.insert(updated_index);

                        continue;
                    }
                }

                invoices_acc.invoices_map.insert(
                    updated_index,
                    Invoices {
                        paid_at: invoice.paid_at.unwrap(),
                        paid_at_str: timestamp_to_localized_datetime_string(
                            config,
                            invoice.paid_at.unwrap(),
                        )?,
                        label: invoice.label,
                        msats_received,
                        sats_received: rounded_div_u64(msats_received, 1_000),
                        description: invoice.description.unwrap_or_default(),
                        payment_hash: invoice.payment_hash.to_string(),
                        preimage: hex_encode(&invoice.payment_preimage.unwrap().to_vec()),
                    },
                );
            }
        }
    }
    Ok(())
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
