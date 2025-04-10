use std::collections::HashSet;

use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::model::responses::{GetinfoResponse, ListpeerchannelsChannels};
use cln_rpc::ClnRpc;
use cln_rpc::{model::requests::*, primitives::Amount};

use log::debug;
use struct_field_names_as_array::FieldNamesAsArray;
use tabled::grid::records::vec_records::Cell;
use tabled::grid::records::Records;
use tabled::settings::location::ByColumnName;
use tabled::settings::object::{Object, Rows};
use tabled::settings::{Alignment, Format, Modify, Panel, Remove, Width};

use tabled::Table;
use tokio::time::Instant;

use crate::structs::{
    Config, PagingIndex, Pays, PluginState, Totals, MISSING_VALUE, NODE_GOSSIP_MISS,
};
use crate::util::{
    at_or_above_version, get_alias, hex_encode, replace_escaping_chars, sort_columns,
    timestamp_to_localized_datetime_string, u64_to_sat_string,
};

pub async fn recent_pays(
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
    config: &Config,
    peer_channels: &[ListpeerchannelsChannels],
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

    let mut pending_pays = Vec::new();
    let mut pending_hashes = HashSet::new();
    let pays = if at_or_above_version(&getinfo.version, "24.11")? {
        debug!(
            "pay_index: start:{} timestamp:{}",
            pay_index.start, pay_index.timestamp
        );
        pending_pays = rpc
            .call_typed(&ListpaysRequest {
                bolt11: None,
                payment_hash: None,
                status: Some(ListpaysStatus::PENDING),
                index: Some(ListpaysIndex::CREATED),
                start: Some(pay_index.start),
                limit: None,
            })
            .await?
            .pays;
        for chan in peer_channels {
            if let Some(htlcs) = &chan.htlcs {
                for htlc in htlcs {
                    pending_hashes.insert(htlc.payment_hash);
                }
            }
        }
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
        now.elapsed().as_millis()
    );

    pay_index.timestamp = now_utc - config_pays_sec;
    if let Some(last_pay) = pays.last() {
        pay_index.start = last_pay.created_index.unwrap_or(u64::MAX);
    }

    for pay in &pending_pays {
        if let Some(dest) = pay.destination {
            if dest == getinfo.id {
                continue;
            }
        }
        if !pending_hashes.contains(&pay.payment_hash) {
            continue;
        }
        if let Some(c_index) = pay.created_index {
            if c_index < pay_index.start {
                pay_index.start = c_index;
            }
        }
    }

    let mut table = Vec::new();

    let description_wanted = config.pays_columns.contains(&"description".to_owned()) || config.json;
    let destination_wanted = config.pays_columns.contains(&"destination".to_owned()) || config.json;

    for pay in pays.into_iter() {
        if pay.completed_at.unwrap() <= Utc::now().timestamp() as u64 - config_pays_sec {
            continue;
        }
        if let Some(dest) = pay.destination {
            if dest == getinfo.id {
                continue;
            }
        }

        let mut fee_msats = None;
        let mut fee_sats = None;
        let mut msats_requested = pay.amount_msat.map(|a| a.msat());
        let mut sats_requested = None;
        let mut description = pay.description;
        let mut destination = pay.destination;
        let mut destination_alias = None;

        if msats_requested.is_none()
            || (description.is_none() && description_wanted)
            || (destination.is_none() && destination_wanted)
        {
            if let Some(b11) = pay.bolt11 {
                if let Ok(invoice) = rpc.call_typed(&DecodeRequest { string: b11 }).await {
                    description = invoice.description;
                    msats_requested = invoice.amount_msat.map(|a| a.msat());
                    destination = invoice.payee;
                }
            } else if let Some(b12) = pay.bolt12 {
                if let Ok(invoice) = rpc.call_typed(&DecodeRequest { string: b12 }).await {
                    description = invoice.offer_description;
                    msats_requested = invoice.invoice_amount_msat.map(|a| a.msat());
                    destination = invoice.invoice_node_id;
                }
            }
        }

        if let Some(dest) = destination {
            if dest == getinfo.id {
                continue;
            }
            destination_alias = Some(get_alias(rpc, plugin.clone(), dest).await?)
        }

        if let Some(amount_msat) = msats_requested {
            fee_msats = Some(pay.amount_sent_msat.unwrap().msat() - amount_msat);
            fee_sats = Some(((fee_msats.unwrap() as f64) / 1_000.0).round() as u64);
            sats_requested = Some(((amount_msat as f64) / 1_000.0).round() as u64);

            if let Some(fee_amt) = &mut totals.pays_fees_msat {
                *fee_amt += fee_msats.unwrap()
            } else {
                totals.pays_fees_msat = fee_msats
            }

            if let Some(pay_amt) = &mut totals.pays_amount_msat {
                *pay_amt += amount_msat
            } else {
                totals.pays_amount_msat = Some(amount_msat)
            }
        };

        if let Some(pay_amt_sent) = &mut totals.pays_amount_sent_msat {
            *pay_amt_sent += pay.amount_sent_msat.unwrap().msat()
        } else {
            totals.pays_amount_sent_msat = Some(pay.amount_sent_msat.unwrap().msat())
        }

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
            destination: if let Some(dest) = &destination_alias {
                if dest == NODE_GOSSIP_MISS {
                    Some(destination.unwrap().to_string())
                } else if config.utf8 {
                    destination_alias
                } else {
                    Some(dest.replace(|c: char| !c.is_ascii(), "?"))
                }
            } else {
                None
            },
            description: description.map(|s| replace_escaping_chars(&s)),
            preimage: hex_encode(&pay.preimage.unwrap().to_vec()),
            msats_requested,
            sats_requested,
            fee_msats,
            fee_sats,
        });

        if let Some(c_index) = pay.created_index {
            if c_index < pay_index.start {
                pay_index.start = c_index;
            }
        }
    }
    if pay_index.start < u64::MAX {
        *plugin.state().pay_index.lock() = pay_index;
    }
    debug!("Build pays table. Total: {}ms", now.elapsed().as_millis());
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
        if !config.pays_columns.contains(&head.to_owned()) {
            paystable.with(Remove::column(ByColumnName::new(head)));
        }
    }
    let headers = paystable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| s.text().to_owned())
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
            |s| {
                if s.eq_ignore_ascii_case(MISSING_VALUE) {
                    s.to_owned()
                } else {
                    u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
                }
            },
        )),
    );
    paystable.with(Modify::new(ByColumnName::new("msats_requested")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("msats_requested").not(Rows::first())).with(Format::content(
            |s| {
                if s.eq_ignore_ascii_case(MISSING_VALUE) {
                    s.to_owned()
                } else {
                    u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
                }
            },
        )),
    );

    paystable.with(Modify::new(ByColumnName::new("fee_sats")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("fee_sats").not(Rows::first())).with(Format::content(|s| {
            if s.eq_ignore_ascii_case(MISSING_VALUE) {
                s.to_owned()
            } else {
                u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
            }
        })),
    );
    paystable.with(Modify::new(ByColumnName::new("fee_msats")).with(Alignment::right()));
    paystable.with(
        Modify::new(ByColumnName::new("fee_msats").not(Rows::first())).with(Format::content(|s| {
            if s.eq_ignore_ascii_case(MISSING_VALUE) {
                s.to_owned()
            } else {
                u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
            }
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
            "off".to_owned()
        }
    )));
    paystable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if totals.pays_amount_sent_msat.is_some() {
        let pays_totals = format!(
            "\nTotal pays stats in the last {}h: {} sats_requested {} sats_sent {} fee_sats",
            config.pays,
            if let Some(amt) = totals.pays_amount_msat {
                u64_to_sat_string(config, ((amt as f64) / 1000.0).round() as u64)?
            } else {
                MISSING_VALUE.to_owned()
            },
            u64_to_sat_string(
                config,
                ((totals.pays_amount_sent_msat.unwrap() as f64) / 1000.0).round() as u64
            )?,
            if let Some(fee) = totals.pays_fees_msat {
                u64_to_sat_string(config, ((fee as f64) / 1000.0).round() as u64)?
            } else {
                MISSING_VALUE.to_owned()
            },
        );
        paystable.with(Panel::footer(pays_totals));
    }

    Ok(paystable.to_string())
}
