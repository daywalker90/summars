use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::model::responses::GetinfoResponse;
use cln_rpc::ClnRpc;
use cln_rpc::{model::requests::*, primitives::Amount};

use log::debug;
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::grid::records::vec_records::Cell;
use tabled::grid::records::Records;
use tabled::settings::location::ByColumnName;
use tabled::settings::object::{Object, Rows};
use tabled::settings::{Alignment, Disable, Format, Modify, Panel, Width};

use tabled::Table;
use tokio::time::Instant;

use crate::structs::{Config, PagingIndex, Pays, PluginState, Totals, NODE_GOSSIP_MISS};
use crate::util::{
    at_or_above_version, get_alias, hex_encode, sort_columns,
    timestamp_to_localized_datetime_string, u64_to_sat_string,
};

pub async fn recent_pays(
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
    config: &Config,
    totals: &mut Totals,
    now: Instant,
    getinfo: &GetinfoResponse,
) -> Result<Vec<Pays>, Error> {
    let now_utc = Utc::now().timestamp() as u64;
    let config_pays_sec = config.pays * 60 * 60;
    {
        if plugin.state().pay_index.lock().timestamp > now_utc - config_pays_sec {
            *plugin.state().pay_index.lock() = PagingIndex::new();
            debug!("pay_index: pays-age increased, resetting index");
        }
    }
    let mut pay_index = plugin.state().pay_index.lock().clone();
    debug!(
        "pay_index: start:{} timestamp:{}",
        pay_index.start, pay_index.timestamp
    );

    let pays = if at_or_above_version(&getinfo.version, "24.11")? {
        rpc.call_typed(&ListpaysRequest {
            bolt11: None,
            payment_hash: None,
            status: Some(ListpaysStatus::COMPLETE),
            index: Some(ListpaysIndex::CREATED),
            start: Some(pay_index.start),
            limit: None,
        })
        .await?
        .pays
    } else {
        rpc.call_typed(&ListpaysRequest {
            bolt11: None,
            payment_hash: None,
            status: Some(ListpaysStatus::COMPLETE),
            index: None,
            start: None,
            limit: None,
        })
        .await?
        .pays
    };

    debug!(
        "List {} pays. Total: {}ms",
        pays.len(),
        now.elapsed().as_millis().to_string()
    );

    pay_index.timestamp = now_utc - config_pays_sec;
    if let Some(last_pay) = pays.last() {
        pay_index.start = last_pay.created_index.unwrap_or(u64::MAX);
    }

    let mut table = Vec::new();

    for pay in pays.into_iter() {
        if pay.completed_at.unwrap() > Utc::now().timestamp() as u64 - config_pays_sec
            && pay.destination.unwrap() != getinfo.id
        {
            let fee_msat = pay.amount_sent_msat.unwrap().msat() - pay.amount_msat.unwrap().msat();

            if let Some(pay_amt) = &mut totals.pays_amount_msat {
                *pay_amt += pay.amount_msat.unwrap().msat()
            } else {
                totals.pays_amount_msat = Some(pay.amount_msat.unwrap().msat())
            }
            if let Some(pay_amt_sent) = &mut totals.pays_amount_sent_msat {
                *pay_amt_sent += pay.amount_sent_msat.unwrap().msat()
            } else {
                totals.pays_amount_sent_msat = Some(pay.amount_sent_msat.unwrap().msat())
            }
            if let Some(fee_amt) = &mut totals.pays_fees_msat {
                *fee_amt += fee_msat
            } else {
                totals.pays_fees_msat = Some(fee_msat)
            }

            let destination = get_alias(rpc, plugin.clone(), pay.destination.unwrap()).await?;
            table.push(Pays {
                completed_at: pay.completed_at.unwrap(),
                completed_at_str: timestamp_to_localized_datetime_string(
                    config,
                    pay.completed_at.unwrap(),
                )?,
                payment_hash: pay.payment_hash.to_string(),
                msats_sent: Amount::msat(&pay.amount_sent_msat.unwrap()),
                sats_sent: ((Amount::msat(&pay.amount_sent_msat.unwrap()) as f64) / 1_000.0).round()
                    as u64,
                destination: if destination == NODE_GOSSIP_MISS {
                    pay.destination.unwrap().to_string()
                } else if config.utf8 {
                    destination
                } else {
                    destination.replace(|c: char| !c.is_ascii(), "?")
                },
                description: if config.pays_columns.contains(&"description".to_string())
                    && !config.json
                {
                    if let Some(desc) = pay.description {
                        desc
                    } else if let Some(b11) = pay.bolt11 {
                        let invoice = rpc.call_typed(&DecodeRequest { string: b11 }).await?;
                        invoice.description.unwrap_or_default()
                    } else {
                        let b12 = pay
                            .bolt12
                            .ok_or_else(|| anyhow!("No description, bolt11 or bolt12 found"))?;
                        let invoice = rpc.call_typed(&DecodeRequest { string: b12 }).await?;
                        invoice.offer_description.unwrap_or_default()
                    }
                } else {
                    String::new()
                },
                preimage: hex_encode(&pay.preimage.unwrap().to_vec()),
                msats_requested: Amount::msat(&pay.amount_msat.unwrap()),
                sats_requested: ((Amount::msat(&pay.amount_msat.unwrap()) as f64) / 1_000.0).round()
                    as u64,
                fee_msats: fee_msat,
                fee_sats: ((fee_msat as f64) / 1_000.0).round() as u64,
            });

            if let Some(c_index) = pay.created_index {
                if c_index < pay_index.start {
                    pay_index.start = c_index;
                }
            }
        }
    }
    if pay_index.start < u64::MAX {
        *plugin.state().pay_index.lock() = pay_index;
    }
    debug!(
        "Build pays table. Total: {}ms",
        now.elapsed().as_millis().to_string()
    );
    if config.pays_limit > 0 && (table.len() as u64) > config.pays_limit {
        table = table.split_off(table.len() - (config.pays_limit as usize))
    }
    table.sort_by_key(|x| x.completed_at);
    Ok(table)
}

pub fn format_pays(table: Vec<Pays>, config: &Config, totals: &Totals) -> Result<String, Error> {
    let mut paystable = Table::new(table);
    config.flow_style.apply(&mut paystable);
    for head in Pays::FIELD_NAMES_AS_ARRAY {
        if !config.pays_columns.contains(&head.to_string()) {
            paystable.with(Disable::column(ByColumnName::new(head)));
        }
    }
    let headers = paystable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_string())
        .collect::<Vec<String>>();
    let records = paystable.get_records_mut();
    if headers.len() != config.pays_columns.len() {
        return Err(anyhow!(
            "Error formatting pays! Length difference detected: {} {}",
            headers.join(","),
            config.pays_columns.join(",")
        ));
    }
    sort_columns(records, &headers, &config.pays_columns);

    if config.max_alias_length < 0 {
        paystable.with(
            Modify::new(ByColumnName::new("destination")).with(
                Width::wrap(config.max_alias_length.unsigned_abs() as usize).keep_words(true),
            ),
        );
    } else {
        paystable.with(
            Modify::new(ByColumnName::new("destination"))
                .with(Width::truncate(config.max_alias_length as usize).suffix("[..]")),
        );
    }

    paystable.with(Modify::new(ByColumnName::new("sats_sent")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("sats_sent").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    paystable.with(Modify::new(ByColumnName::new("msats_sent")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("msats_sent").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );

    paystable.with(Modify::new(ByColumnName::new("sats_requested")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("sats_requested").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );
    paystable.with(Modify::new(ByColumnName::new("msats_requested")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("msats_requested").not(Rows::first())).with(Format::content(
            |s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap(),
        )),
    );

    paystable.with(Modify::new(ByColumnName::new("fee_sats")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("fee_sats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );
    paystable.with(Modify::new(ByColumnName::new("fee_msats")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("fee_msats").not(Rows::first())).with(Format::content(|s| {
            u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
        })),
    );

    if config.max_desc_length < 0 {
        paystable
            .with(Modify::new(ByColumnName::new("description")).with(
                Width::wrap(config.max_desc_length.unsigned_abs() as usize).keep_words(true),
            ));
    } else {
        paystable.with(
            Modify::new(ByColumnName::new("description"))
                .with(Width::truncate(config.max_desc_length as usize).suffix("[..]")),
        );
    }

    paystable.with(Panel::header(format!(
        "pays (last {}h, limit: {})",
        config.pays,
        if config.pays_limit > 0 {
            config.pays_limit.to_string()
        } else {
            "off".to_string()
        }
    )));
    paystable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if totals.pays_amount_msat.is_some() {
        let pays_totals = format!(
            "\nTotal pays stats in the last {}h: {} sats_requested {} sats_sent {} fee_sats",
            config.pays,
            u64_to_sat_string(
                config,
                ((totals.pays_amount_msat.unwrap() as f64) / 1000.0).round() as u64
            )?,
            u64_to_sat_string(
                config,
                ((totals.pays_amount_sent_msat.unwrap() as f64) / 1000.0).round() as u64
            )?,
            u64_to_sat_string(
                config,
                ((totals.pays_fees_msat.unwrap() as f64) / 1000.0).round() as u64
            )?,
        );
        paystable.with(Panel::footer(pays_totals));
    }

    Ok(paystable.to_string())
}
