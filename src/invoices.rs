use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::ClnRpc;
use cln_rpc::{model::requests::*, model::responses::*, primitives::Amount};

use serde_json::json;
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::grid::records::vec_records::Cell;
use tabled::grid::records::Records;
use tabled::settings::location::ByColumnName;
use tabled::settings::object::{Object, Rows};
use tabled::settings::{Alignment, Format, Modify, Panel, Remove, Width};

use tabled::Table;
use tokio::time::Instant;

use crate::structs::{
    Config, HoldLookupResponse, Holdstate, Invoices, InvoicesFilterStats, PagingIndex, PluginState,
    Totals,
};
use crate::util::{
    hex_encode, replace_escaping_chars, sort_columns, timestamp_to_localized_datetime_string,
    u64_to_sat_string,
};

pub async fn recent_invoices(
    plugin: Plugin<PluginState>,
    rpc: &mut ClnRpc,
    config: &Config,
    totals: &mut Totals,
    now: Instant,
) -> Result<(Vec<Invoices>, InvoicesFilterStats), Error> {
    let now_utc = Utc::now().timestamp() as u64;
    let config_invoices_sec = config.invoices * 60 * 60;
    {
        if plugin.state().inv_index.lock().timestamp > now_utc - config_invoices_sec {
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

    inv_index.timestamp = now_utc - config_invoices_sec;
    if let Some(last_inv) = invoices.last() {
        inv_index.start = last_inv.created_index.unwrap_or(u64::MAX);
    }

    let mut table = Vec::new();
    let mut filter_count = 0;
    let mut filter_amt_sum_msat = 0;

    for invoice in invoices.into_iter() {
        if ListinvoicesInvoicesStatus::PAID == invoice.status {
            let inv_paid_at = if let Some(p_at) = invoice.paid_at {
                p_at
            } else {
                continue;
            };
            if inv_paid_at > now_utc - config_invoices_sec {
                if let Some(inv_amt) = &mut totals.invoices_amount_received_msat {
                    *inv_amt += invoice.amount_received_msat.unwrap().msat()
                } else {
                    totals.invoices_amount_received_msat =
                        Some(invoice.amount_received_msat.unwrap().msat())
                }

                if invoice.amount_received_msat.unwrap().msat() as i64
                    <= config.invoices_filter_amt_msat
                {
                    filter_count += 1;
                    filter_amt_sum_msat += invoice.amount_received_msat.unwrap().msat();
                } else {
                    table.push(Invoices {
                        paid_at: invoice.paid_at.unwrap(),
                        paid_at_str: timestamp_to_localized_datetime_string(
                            config,
                            invoice.paid_at.unwrap(),
                        )?,
                        label: invoice.label,
                        msats_received: Amount::msat(&invoice.amount_received_msat.unwrap()),
                        sats_received: ((Amount::msat(&invoice.amount_received_msat.unwrap())
                            as f64)
                            / 1_000.0)
                            .round() as u64,
                        description: invoice.description.unwrap_or_default(),
                        payment_hash: invoice.payment_hash.to_string(),
                        preimage: hex_encode(&invoice.payment_preimage.unwrap().to_vec()),
                    });
                }
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
    if inv_index.start < u64::MAX {
        *plugin.state().inv_index.lock() = inv_index;
    }
    log::debug!(
        "Build invoices table entries. Total: {}ms",
        now.elapsed().as_millis()
    );

    if plugin.state().config.lock().hold_invoice_support {
        let holdinvoices: HoldLookupResponse =
            rpc.call_raw("holdinvoicelookup", &json!({})).await?;

        for holdinvoice in holdinvoices.holdinvoices.into_iter() {
            if holdinvoice.state == Holdstate::Settled {
                let paid_at = holdinvoice.paid_at.unwrap();
                if paid_at > now_utc - config_invoices_sec {
                    if let Some(inv_amt) = &mut totals.invoices_amount_received_msat {
                        *inv_amt += holdinvoice.amount_msat
                    } else {
                        totals.invoices_amount_received_msat = Some(holdinvoice.amount_msat)
                    }

                    if holdinvoice.amount_msat as i64 <= config.invoices_filter_amt_msat {
                        filter_count += 1;
                        filter_amt_sum_msat += holdinvoice.amount_msat;
                    } else {
                        table.push(Invoices {
                            paid_at,
                            paid_at_str: timestamp_to_localized_datetime_string(config, paid_at)?,
                            label: "Holdinvoice".to_owned(),
                            msats_received: holdinvoice.amount_msat,
                            sats_received: ((holdinvoice.amount_msat as f64) / 1_000.0).round()
                                as u64,
                            description: holdinvoice.description.unwrap_or_default(),
                            payment_hash: holdinvoice.payment_hash,
                            preimage: holdinvoice.preimage.unwrap_or_default(),
                        });
                    }
                }
            }
        }
        log::debug!(
            "Build holdinvoices table entries. Total: {}ms",
            now.elapsed().as_millis()
        );
    }

    table.sort_by_key(|x| x.paid_at);

    if config.invoices_limit > 0 && (table.len() as u64) > config.invoices_limit {
        let start = table.len().saturating_sub(config.invoices_limit as usize);
        table = table.drain(start..).collect();
    }

    Ok((
        table,
        InvoicesFilterStats {
            filter_amt_sum_msat,
            filter_count,
        },
    ))
}

pub fn format_invoices(
    table: Vec<Invoices>,
    config: &Config,
    totals: &Totals,
    filter_stats: InvoicesFilterStats,
) -> Result<String, Error> {
    let mut invoicestable = Table::new(table);
    config.flow_style.apply(&mut invoicestable);
    for head in Invoices::FIELD_NAMES_AS_ARRAY {
        if !config.invoices_columns.contains(&head.to_owned()) {
            invoicestable.with(Remove::column(ByColumnName::new(head)));
        }
    }
    let headers = invoicestable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_owned())
        .collect::<Vec<String>>();
    let records = invoicestable.get_records_mut();
    if headers.len() != config.invoices_columns.len() {
        return Err(anyhow!(
            "Error formatting invoices! Length difference detected: {} {}",
            headers.join(","),
            config.invoices_columns.join(",")
        ));
    }
    sort_columns(records, &headers, &config.invoices_columns);

    if config.max_desc_length < 0 {
        invoicestable.with(
            Modify::new(ByColumnName::new("description"))
                .with(Format::content(replace_escaping_chars))
                .with(Width::wrap(config.max_desc_length.unsigned_abs() as usize).keep_words(true)),
        );
    } else {
        invoicestable.with(
            Modify::new(ByColumnName::new("description"))
                .with(Format::content(replace_escaping_chars))
                .with(Width::truncate(config.max_desc_length as usize).suffix("[..]")),
        );
    }

    if config.max_label_length < 0 {
        invoicestable.with(
            Modify::new(ByColumnName::new("label")).with(
                Width::wrap(config.max_label_length.unsigned_abs() as usize).keep_words(true),
            ),
        );
    } else {
        invoicestable.with(
            Modify::new(ByColumnName::new("label"))
                .with(Width::truncate(config.max_label_length as usize).suffix("[..]")),
        );
    }

    invoicestable.with(Modify::new(ByColumnName::new("sats_received")).with(Alignment::right()));
    invoicestable.with(
        Modify::new(ByColumnName::new("sats_received").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );
    invoicestable.with(Modify::new(ByColumnName::new("msats_received")).with(Alignment::right()));
    invoicestable.with(
        Modify::new(ByColumnName::new("msats_received").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );

    invoicestable.with(Panel::header(format!(
        "invoices (last {}h, limit: {})",
        config.invoices,
        if config.invoices_limit > 0 {
            config.invoices_limit.to_string()
        } else {
            "off".to_owned()
        }
    )));
    invoicestable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if filter_stats.filter_count > 0 {
        let filter_sum_result = format!(
            "\nFiltered {} invoice{} with {} sats total.",
            filter_stats.filter_count,
            if filter_stats.filter_count == 1 {
                ""
            } else {
                "s"
            },
            u64_to_sat_string(
                config,
                ((filter_stats.filter_amt_sum_msat as f64) / 1_000.0).round() as u64
            )?
        );
        invoicestable.with(Panel::footer(filter_sum_result));
    }

    if let Some(inv_total) = totals.invoices_amount_received_msat {
        let invoices_total = format!(
            "\nTotal invoices stats in the last {}h: {} sats_received",
            config.invoices,
            u64_to_sat_string(config, ((inv_total as f64) / 1000.0).round() as u64)?,
        );
        invoicestable.with(Panel::footer(invoices_total));
    }
    Ok(invoicestable.to_string())
}
