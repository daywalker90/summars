use std::collections::HashSet;

use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::{
    model::{
        requests::{DecodeRequest, ListpaysIndex, ListpaysRequest, ListpaysStatus},
        responses::{ListpaysPays, ListpeerchannelsChannels},
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
        PagingIndex,
        Pays,
        PaysColumns,
        PluginState,
        TableColumn,
        MISSING_VALUE,
        NODE_GOSSIP_MISS,
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

pub async fn gather_pays_data(
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
    config: &Config,
    peer_channels: &[ListpeerchannelsChannels],
    now: Instant,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let now_utc = Utc::now().timestamp().unsigned_abs();
    let config_pays_sec = config.pays * 60 * 60;
    let cutoff_timestamp = now_utc - config_pays_sec;
    {
        if plugin.state().pay_index.lock().timestamp > cutoff_timestamp {
            *plugin.state().pay_index.lock() = PagingIndex::new();
            log::debug!("pay_index: pays-age increased, resetting index");
        }
    }
    let mut pay_index = plugin.state().pay_index.lock().clone();

    log::debug!(
        "pay_index: start:{} timestamp:{}",
        pay_index.start,
        pay_index.timestamp
    );
    let pays = rpc
        .call_typed(&ListpaysRequest {
            bolt11: None,
            payment_hash: None,
            status: Some(ListpaysStatus::COMPLETE),
            index: Some(ListpaysIndex::CREATED),
            start: Some(pay_index.start),
            limit: None,
        })
        .await?
        .pays;

    let pending_pays = rpc
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

    let mut pending_hashes = HashSet::new();
    for chan in peer_channels {
        if let Some(htlcs) = &chan.htlcs {
            for htlc in htlcs {
                pending_hashes.insert(htlc.payment_hash);
            }
        }
    }

    log::debug!(
        "List {} pays. Total: {}ms",
        pays.len(),
        now.elapsed().as_millis()
    );

    pay_index.timestamp = cutoff_timestamp;
    if let Some(last_pay) = pays.last() {
        pay_index.start = last_pay.created_index.unwrap_or(u64::MAX);
    }

    for pay in &pending_pays {
        if let Some(dest) = pay.destination {
            if dest == full_node_data.my_pubkey {
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

    build_pays_table(
        config,
        pays,
        &mut pay_index,
        full_node_data,
        rpc,
        plugin.clone(),
    )
    .await?;

    if pay_index.start < u64::MAX {
        *plugin.state().pay_index.lock() = pay_index;
    }
    log::debug!("Build pays table. Total: {}ms", now.elapsed().as_millis());
    if config.pays_limit > 0 && full_node_data.pays.len() > config.pays_limit {
        full_node_data.pays = full_node_data
            .pays
            .split_off(full_node_data.pays.len() - config.pays_limit);
    }
    full_node_data.pays.sort_by_key(|x| x.completed_at);

    Ok(())
}

async fn build_pays_table(
    config: &Config,
    pays: Vec<ListpaysPays>,
    pay_index: &mut PagingIndex,
    full_node_data: &mut FullNodeData,
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
) -> Result<(), Error> {
    let description_wanted = config.pays_columns.contains(&PaysColumns::description) || config.json;
    let destination_wanted = config.pays_columns.contains(&PaysColumns::destination) || config.json;

    for pay in pays {
        if pay.completed_at.unwrap() <= pay_index.timestamp {
            continue;
        }
        if let Some(dest) = pay.destination {
            if dest == full_node_data.my_pubkey {
                continue;
            }
        }

        let mut fee_msat = None;
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
            if dest == full_node_data.my_pubkey {
                continue;
            }
            destination_alias = plugin.state().alias_map.lock().get(&dest).cloned();
        }

        if let Some(amount_msat) = msats_requested {
            fee_msat = Some(pay.amount_sent_msat.unwrap().msat() - amount_msat);
            fee_sats = Some(rounded_div_u64(fee_msat.unwrap(), 1_000));
            sats_requested = Some(rounded_div_u64(amount_msat, 1_000));

            if let Some(fee_amt) = &mut full_node_data.totals.pays_fees_msat {
                *fee_amt += fee_msat.unwrap();
            } else {
                full_node_data.totals.pays_fees_msat = fee_msat;
            }

            if let Some(pay_amt) = &mut full_node_data.totals.pays_amount_msat {
                *pay_amt += amount_msat;
            } else {
                full_node_data.totals.pays_amount_msat = Some(amount_msat);
            }
        }

        if let Some(pay_amt_sent) = &mut full_node_data.totals.pays_amount_sent_msat {
            *pay_amt_sent += pay.amount_sent_msat.unwrap().msat();
        } else {
            full_node_data.totals.pays_amount_sent_msat =
                Some(pay.amount_sent_msat.unwrap().msat());
        }

        full_node_data.pays.push(Pays {
            completed_at: pay.completed_at.unwrap(),
            completed_at_str: timestamp_to_localized_datetime_string(
                config,
                pay.completed_at.unwrap(),
            )?,
            payment_hash: pay.payment_hash.to_string(),
            msats_sent: Amount::msat(&pay.amount_sent_msat.unwrap()),
            sats_sent: rounded_div_u64(pay.amount_sent_msat.unwrap().msat(), 1_000),
            destination: if let Some(dest) = destination_alias {
                if dest == NODE_GOSSIP_MISS {
                    Some(destination.unwrap().to_string())
                } else if config.utf8 {
                    Some(dest)
                } else {
                    Some(dest.replace(|c: char| !c.is_ascii(), "?"))
                }
            } else {
                None
            },
            description,
            preimage: hex_encode(&pay.preimage.unwrap().to_vec()),
            msats_requested,
            sats_requested,
            fee_msats: fee_msat,
            fee_sats,
        });

        if let Some(c_index) = pay.created_index {
            if c_index < pay_index.start {
                pay_index.start = c_index;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub fn format_pays(config: &Config, full_node_data: &mut FullNodeData) -> Result<String, Error> {
    let count = full_node_data.pays.len();
    let mut paystable = Table::new(&full_node_data.pays);
    config.flow_style.apply(&mut paystable);
    for head in PaysColumns::iter() {
        if !config.pays_columns.contains(&head) {
            paystable.with(Remove::column(ByColumnName::new(head.to_string())));
        }
    }
    let headers = paystable
        .get_records()
        .iter_rows()
        .next()
        .unwrap()
        .iter()
        .map(|s| PaysColumns::parse_column(s.text()).unwrap())
        .collect::<Vec<PaysColumns>>();
    let records = paystable.get_records_mut();
    if headers.len() != config.pays_columns.len() {
        return Err(anyhow!(
            "Error formatting pays! Length difference detected: {} {}",
            PaysColumns::to_list_string(&headers),
            PaysColumns::to_list_string(&config.pays_columns)
        ));
    }
    sort_columns(records, &headers, &config.pays_columns);

    for numerical in PaysColumns::NUMERICAL {
        paystable
            .with(Modify::new(ByColumnName::new(numerical.to_string())).with(Alignment::right()));
        paystable.with(
            Modify::new(ByColumnName::new(numerical.to_string()).not(Rows::first())).with(
                Format::content(|s| u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()),
            ),
        );
    }

    for opt_num in PaysColumns::OPTIONAL_NUMERICAL {
        paystable
            .with(Modify::new(ByColumnName::new(opt_num.to_string())).with(Alignment::right()));
        paystable.with(
            Modify::new(ByColumnName::new(opt_num.to_string()).not(Rows::first())).with(
                Format::content(|s| {
                    if s.eq_ignore_ascii_case(MISSING_VALUE) {
                        s.to_owned()
                    } else {
                        u64_to_sat_string(config, s.parse::<u64>().unwrap()).unwrap()
                    }
                }),
            ),
        );
    }

    if config.max_alias_length < 0 {
        paystable.with(
            Modify::new(ByColumnName::new(PaysColumns::destination.to_string())).with(
                Width::wrap(usize::try_from(config.max_alias_length.unsigned_abs())?)
                    .keep_words(true),
            ),
        );
    } else {
        paystable.with(
            Modify::new(ByColumnName::new(PaysColumns::destination.to_string()))
                .with(Width::truncate(usize::try_from(config.max_alias_length)?).suffix("[..]")),
        );
    }
    if config.max_desc_length < 0 {
        paystable.with(
            Modify::new(ByColumnName::new(PaysColumns::description.to_string()))
                .with(Format::content(replace_escaping_chars))
                .with(
                    Width::wrap(usize::try_from(config.max_desc_length.unsigned_abs())?)
                        .keep_words(true),
                ),
        );
    } else {
        paystable.with(
            Modify::new(ByColumnName::new(PaysColumns::description.to_string()))
                .with(Format::content(replace_escaping_chars))
                .with(Width::truncate(usize::try_from(config.max_desc_length)?).suffix("[..]")),
        );
    }

    paystable.with(Panel::header(format!(
        "pays (last {}h, limit: {})",
        config.pays,
        if config.pays_limit > 0 {
            format!("{}/{}", count, config.pays_limit)
        } else {
            "off".to_owned()
        }
    )));
    paystable.with(Modify::new(Rows::first()).with(Alignment::center()));

    if full_node_data.totals.pays_amount_sent_msat.is_some() {
        let pays_totals = format!(
            "\nTotal pays stats in the last {}h: {} sats_requested {} sats_sent {} fee_sats",
            config.pays,
            if let Some(amt) = full_node_data.totals.pays_amount_msat {
                u64_to_sat_string(config, rounded_div_u64(amt, 1_000))?
            } else {
                MISSING_VALUE.to_owned()
            },
            u64_to_sat_string(
                config,
                rounded_div_u64(full_node_data.totals.pays_amount_sent_msat.unwrap(), 1_000)
            )?,
            if let Some(fee) = full_node_data.totals.pays_fees_msat {
                u64_to_sat_string(config, rounded_div_u64(fee, 1000))?
            } else {
                MISSING_VALUE.to_owned()
            },
        );
        paystable.with(Panel::footer(pays_totals));
    }

    Ok(paystable.to_string())
}
