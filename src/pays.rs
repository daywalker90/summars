use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
};

use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::Plugin;
use cln_rpc::{
    model::{
        requests::{
            DecodeRequest,
            ListpaysIndex,
            ListpaysRequest,
            ListpaysStatus,
            WaitIndexname,
            WaitRequest,
            WaitSubsystem,
        },
        responses::ListpaysPays,
    },
    primitives::{Amount, PublicKey},
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
        Pays,
        PaysColumns,
        PluginState,
        TableColumn,
        MISSING_VALUE,
        NODE_GOSSIP_MISS,
        NO_ALIAS_SET,
        PAGE_SIZE,
    },
    util::{
        accumulate_msat,
        get_alias,
        hex_encode,
        replace_escaping_chars,
        rounded_div_u64,
        sort_columns,
        timestamp_to_localized_datetime_string,
        u64_to_sat_string,
    },
};

struct PaysAccumulator {
    oldest_updated: u64,
    cutoff_timestamp: u64,
    pays_map: BTreeMap<u64, Pays>,
    filtered_set: HashSet<u64>,
}

pub async fn gather_pays_data(
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
    config: &Config,
    now: Instant,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let now_utc = Utc::now().timestamp().unsigned_abs();
    let config_pays_sec = config.pays * 60 * 60;
    let cutoff_timestamp = now_utc - config_pays_sec;

    let oldest_updated = now_utc;

    let pays_map: BTreeMap<u64, Pays> = BTreeMap::new();

    let filtered_set: HashSet<u64> = HashSet::new();

    let mut pays_acc = PaysAccumulator {
        oldest_updated,
        cutoff_timestamp,
        pays_map,
        filtered_set,
    };

    process_pay_batches(
        plugin.clone(),
        now,
        &mut pays_acc,
        config,
        rpc,
        full_node_data,
    )
    .await?;

    limit_and_sort_pays_data(pays_acc, config, full_node_data);

    Ok(())
}

async fn process_pay_batches(
    plugin: Plugin<PluginState>,
    now: Instant,
    pays_acc: &mut PaysAccumulator,
    config: &Config,
    rpc: &mut ClnRpc,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    let mut current_index = rpc
        .call_typed(&WaitRequest {
            indexname: WaitIndexname::UPDATED,
            subsystem: WaitSubsystem::SENDPAYS,
            nextvalue: 0,
        })
        .await?
        .updated
        .unwrap();
    log::debug!("Current pays index: {current_index}");

    let mut loop_count = 0;

    current_index = current_index.saturating_sub(PAGE_SIZE - 1);
    let mut limit = u32::try_from(PAGE_SIZE)?;

    while pays_acc.oldest_updated >= pays_acc.cutoff_timestamp {
        loop_count += 1;

        let pays = rpc
            .call_typed(&ListpaysRequest {
                bolt11: None,
                payment_hash: None,
                status: Some(ListpaysStatus::COMPLETE),
                index: Some(ListpaysIndex::UPDATED),
                start: Some(current_index),
                limit: Some(limit),
            })
            .await?
            .pays;

        build_pays_table(pays_acc, config, pays, full_node_data, rpc, plugin.clone()).await?;

        if current_index <= 1 {
            break;
        }
        limit = u32::min(u32::try_from(PAGE_SIZE)?, u32::try_from(current_index)?);
        current_index = current_index.saturating_sub(PAGE_SIZE);
    }

    log::debug!(
        "Build pays table in {loop_count} calls. Total: {}ms",
        now.elapsed().as_millis()
    );

    Ok(())
}

fn limit_and_sort_pays_data(
    pays_acc: PaysAccumulator,
    config: &Config,
    full_node_data: &mut FullNodeData,
) {
    if config.pays_limit > 0 && pays_acc.pays_map.len() > config.pays_limit {
        full_node_data.pays = pays_acc
            .pays_map
            .into_values()
            .rev()
            .take(config.pays_limit)
            .rev()
            .collect();
    } else {
        full_node_data.pays = pays_acc.pays_map.into_values().collect();
    }

    full_node_data.pays.sort_by_key(|x| x.completed_at);
}

async fn build_pays_table(
    pays_acc: &mut PaysAccumulator,
    config: &Config,
    pays: Vec<ListpaysPays>,
    full_node_data: &mut FullNodeData,
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
) -> Result<(), Error> {
    let description_wanted = config.pays_columns.contains(&PaysColumns::description) || config.json;
    let destination_wanted = config.pays_columns.contains(&PaysColumns::destination) || config.json;

    for pay in pays.into_iter().rev() {
        let Some(updated_index) = pay.updated_index else {
            continue;
        };

        let Some(completed_at) = pay.completed_at else {
            continue;
        };

        if pays_acc.pays_map.contains_key(&updated_index) {
            continue;
        }

        if pays_acc.filtered_set.contains(&updated_index) {
            continue;
        }

        if completed_at <= pays_acc.oldest_updated {
            pays_acc.oldest_updated = completed_at;
        }

        if completed_at <= pays_acc.cutoff_timestamp {
            continue;
        }

        let mut fee_msat = None;
        let mut fee_sats = None;
        let mut sats_requested = None;
        let mut destination_alias = None;

        let (description, msats_requested, destination) =
            extract_pay_metadata(rpc, &pay, description_wanted, destination_wanted).await;

        let mut is_self_pay = false;

        if let Some(dest) = destination {
            if dest == full_node_data.my_pubkey {
                is_self_pay = true;
            }
            destination_alias = Some(get_alias(rpc, plugin.clone(), dest).await?);
        }

        if let Some(amount_msat) = msats_requested {
            fee_msat = Some(pay.amount_sent_msat.unwrap().msat() - amount_msat);
            fee_sats = Some(rounded_div_u64(fee_msat.unwrap(), 1_000));
            sats_requested = Some(rounded_div_u64(amount_msat, 1_000));

            if is_self_pay {
                accumulate_msat(
                    &mut full_node_data.totals.pays_self_fees_msat,
                    fee_msat.unwrap(),
                );
                accumulate_msat(
                    &mut full_node_data.totals.pays_self_amount_msat,
                    amount_msat,
                );
            } else {
                accumulate_msat(&mut full_node_data.totals.pays_fees_msat, fee_msat.unwrap());
                accumulate_msat(&mut full_node_data.totals.pays_amount_msat, amount_msat);
            }
        }

        if is_self_pay {
            accumulate_msat(
                &mut full_node_data.totals.pays_self_amount_sent_msat,
                pay.amount_sent_msat.unwrap().msat(),
            );
        } else {
            accumulate_msat(
                &mut full_node_data.totals.pays_amount_sent_msat,
                pay.amount_sent_msat.unwrap().msat(),
            );
        }

        if is_self_pay {
            pays_acc.filtered_set.insert(updated_index);
        } else {
            pays_acc.pays_map.insert(
                updated_index,
                Pays {
                    completed_at,
                    completed_at_str: timestamp_to_localized_datetime_string(config, completed_at)?,
                    payment_hash: pay.payment_hash.to_string(),
                    msats_sent: Amount::msat(&pay.amount_sent_msat.unwrap()),
                    sats_sent: rounded_div_u64(pay.amount_sent_msat.unwrap().msat(), 1_000),
                    destination: if let Some(dest) = destination_alias {
                        if dest == NODE_GOSSIP_MISS || dest == NO_ALIAS_SET {
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
                },
            );
        }
    }
    Ok(())
}

async fn extract_pay_metadata(
    rpc: &mut ClnRpc,
    pay: &ListpaysPays,
    description_wanted: bool,
    destination_wanted: bool,
) -> (Option<String>, Option<u64>, Option<PublicKey>) {
    let mut description = pay.description.clone();
    let mut msats_requested = pay.amount_msat.map(|a| a.msat());
    let mut destination = pay.destination;

    let needs_invoice_decode = msats_requested.is_none()
        || (description.is_none() && description_wanted)
        || (destination.is_none() && destination_wanted);

    if needs_invoice_decode {
        if let Some(b11) = &pay.bolt11 {
            if let Ok(invoice) = rpc
                .call_typed(&DecodeRequest {
                    string: b11.to_owned(),
                })
                .await
            {
                description = invoice.description;
                msats_requested = invoice.amount_msat.map(|a| a.msat());
                destination = invoice.payee;
            }
        } else if let Some(b12) = &pay.bolt12 {
            if let Ok(invoice) = rpc
                .call_typed(&DecodeRequest {
                    string: b12.to_owned(),
                })
                .await
            {
                description = invoice.offer_description;
                msats_requested = invoice.invoice_amount_msat.map(|a| a.msat());
                destination = invoice.invoice_node_id;
            }
        }
    }

    (description, msats_requested, destination)
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

    let mut pays_totals = String::new();

    if full_node_data.totals.pays_amount_sent_msat.is_some() {
        write!(
            pays_totals,
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
        )?;
    }

    if full_node_data.totals.pays_self_amount_sent_msat.is_some() {
        write!(
            pays_totals,
            "\nTotal self-pays stats in the last {}h: {} sats_requested {} sats_sent {} fee_sats",
            config.pays,
            if let Some(amt) = full_node_data.totals.pays_self_amount_msat {
                u64_to_sat_string(config, rounded_div_u64(amt, 1_000))?
            } else {
                MISSING_VALUE.to_owned()
            },
            u64_to_sat_string(
                config,
                rounded_div_u64(
                    full_node_data.totals.pays_self_amount_sent_msat.unwrap(),
                    1_000
                )
            )?,
            if let Some(fee) = full_node_data.totals.pays_self_fees_msat {
                u64_to_sat_string(config, rounded_div_u64(fee, 1000))?
            } else {
                MISSING_VALUE.to_owned()
            },
        )?;
    }

    if !pays_totals.is_empty() {
        paystable.with(Panel::footer(pays_totals));
    }

    Ok(paystable.to_string())
}
