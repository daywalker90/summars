use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::{
    model::{
        requests::{ListinvoicesIndex, ListinvoicesRequest},
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
    {
        if plugin.state().inv_index.lock().timestamp > cutoff_timestamp {
            *plugin.state().inv_index.lock() = PagingIndex::new();
            log::debug!("inv_index: invoices-age increased, resetting index");
        }
    }
    let mut inv_index = plugin.state().inv_index.lock().clone();
    log::debug!(
        "inv_index: start:{} timestamp:{}",
        inv_index.start,
        inv_index.timestamp
    );
    let invoices = rpc
        .call_typed(&ListinvoicesRequest {
            label: None,
            invstring: None,
            payment_hash: None,
            offer_id: None,
            index: Some(ListinvoicesIndex::CREATED),
            start: Some(inv_index.start),
            limit: None,
        })
        .await?
        .invoices;
    log::debug!(
        "List {} invoices. Total: {}ms",
        invoices.len(),
        now.elapsed().as_millis()
    );

    inv_index.timestamp = cutoff_timestamp;
    if let Some(last_inv) = invoices.last() {
        inv_index.start = last_inv.created_index.unwrap_or(u64::MAX);
    }

    build_invoices_table(invoices, &mut inv_index, full_node_data, config)?;

    if inv_index.start < u64::MAX {
        *plugin.state().inv_index.lock() = inv_index;
    }
    log::debug!(
        "Build invoices table. Total: {}ms",
        now.elapsed().as_millis()
    );
    if config.invoices_limit > 0 && full_node_data.invoices.len() > config.invoices_limit {
        full_node_data.invoices = full_node_data
            .invoices
            .split_off(full_node_data.invoices.len() - config.invoices_limit);
    }
    full_node_data.invoices.sort_by_key(|x| x.paid_at);

    Ok(())
}

fn build_invoices_table(
    invoices: Vec<ListinvoicesInvoices>,
    inv_index: &mut PagingIndex,
    full_node_data: &mut FullNodeData,
    config: &Config,
) -> Result<(), Error> {
    for invoice in invoices {
        if ListinvoicesInvoicesStatus::PAID == invoice.status {
            let Some(inv_paid_at) = invoice.paid_at else {
                continue;
            };
            if inv_paid_at > inv_index.timestamp {
                if let Some(inv_amt) = &mut full_node_data.totals.invoices_amount_received_msat {
                    *inv_amt += invoice.amount_received_msat.unwrap().msat();
                } else {
                    full_node_data.totals.invoices_amount_received_msat =
                        Some(invoice.amount_received_msat.unwrap().msat());
                }

                if let Some(if_amt) = config.invoices_filter_amt_msat {
                    if invoice.amount_received_msat.unwrap().msat() <= if_amt {
                        full_node_data.invoices_filter_stats.filter_count += 1;
                        full_node_data.invoices_filter_stats.filter_amt_sum_msat +=
                            invoice.amount_received_msat.unwrap().msat();
                        continue;
                    }
                }

                full_node_data.invoices.push(Invoices {
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
                });

                if let Some(c_index) = invoice.created_index {
                    if c_index < inv_index.start {
                        inv_index.start = c_index;
                    }
                }
            }
        } else if ListinvoicesInvoicesStatus::UNPAID == invoice.status {
            if let Some(c_index) = invoice.created_index {
                if c_index < inv_index.start {
                    inv_index.start = c_index;
                }
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
