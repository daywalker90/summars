use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
    str::FromStr,
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
    primitives::Amount,
    ClnRpc,
};
use lightning_invoice::Bolt11Invoice;
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
        DescriptionStatus,
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

    process_pay_batches(&plugin, now, &mut pays_acc, config, rpc, full_node_data).await?;

    limit_and_sort_pays_data(pays_acc, config, full_node_data);

    let description_wanted = config.pays_columns.contains(&PaysColumns::description) || config.json;

    if description_wanted {
        extract_pay_descriptions(now, rpc, full_node_data).await?;
    }

    Ok(())
}

async fn process_pay_batches(
    plugin: &Plugin<PluginState>,
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

    let first_index = rpc
        .call_typed(&ListpaysRequest {
            bolt11: None,
            payment_hash: None,
            status: Some(ListpaysStatus::COMPLETE),
            index: Some(ListpaysIndex::UPDATED),
            start: Some(0),
            limit: Some(1),
        })
        .await?
        .pays
        .first()
        .and_then(|p| p.updated_index)
        .unwrap_or(0);
    log::debug!("Current pays index: {current_index}, first index: {first_index}");

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

        build_pays_table(pays_acc, config, pays, full_node_data, rpc, plugin).await?;

        if current_index <= 1 || current_index <= first_index {
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
    plugin: &Plugin<PluginState>,
) -> Result<(), Error> {
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
        let mut msats_requested = None;
        let mut sats_requested = None;
        let mut destination_alias = None;

        let mut is_self_pay = false;

        if let Some(dest) = pay.destination {
            if dest == full_node_data.my_pubkey {
                is_self_pay = true;
            }
            destination_alias = Some(get_alias(rpc, plugin, dest).await?);
        }

        let msats_sent = if let Some(amt_sent) = pay.amount_sent_msat {
            amt_sent.msat()
        } else {
            log::warn!(
                "Completed pay with payment_hash {} has no amount_sent_msat",
                pay.payment_hash
            );
            continue;
        };

        if let Some(amount_msat) = pay.amount_msat.map(|a| Amount::msat(&a)) {
            msats_requested = Some(amount_msat);
            fee_msat = Some(msats_sent.saturating_sub(amount_msat));
            fee_sats = Some(rounded_div_u64(fee_msat.unwrap(), 1_000));
            sats_requested = Some(rounded_div_u64(amount_msat, 1_000));

            accumulate_totals(
                is_self_pay,
                fee_msat,
                full_node_data,
                amount_msat,
                msats_sent,
            );
        }

        let (description_status, description) =
            get_pay_description_and_status(pay.description, pay.bolt11, pay.bolt12);

        if is_self_pay {
            full_node_data.totals.pays.self_count += 1;
            pays_acc.filtered_set.insert(updated_index);
        } else {
            full_node_data.totals.pays.count += 1;
            pays_acc.pays_map.insert(
                updated_index,
                Pays {
                    completed_at,
                    payment_hash: pay.payment_hash.to_string(),
                    msats_sent,
                    sats_sent: rounded_div_u64(msats_sent, 1_000),
                    destination: if let Some(dest) = destination_alias {
                        if dest == NODE_GOSSIP_MISS || dest == NO_ALIAS_SET {
                            Some(pay.destination.unwrap().to_string())
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
                    description_status,
                },
            );
        }
    }
    Ok(())
}

fn get_pay_description_and_status(
    description: Option<String>,
    bolt11: Option<String>,
    bolt12: Option<String>,
) -> (DescriptionStatus, Option<String>) {
    let mut description_status = DescriptionStatus::Processed;
    let description = if let Some(desc) = description {
        Some(desc)
    } else if let Some(b11) = bolt11 {
        description_status = DescriptionStatus::Bolt11;
        Some(b11)
    } else if let Some(b12) = bolt12 {
        description_status = DescriptionStatus::Bolt12;
        Some(b12)
    } else {
        None
    };
    (description_status, description)
}

fn accumulate_totals(
    is_self_pay: bool,
    fee_msat: Option<u64>,
    full_node_data: &mut FullNodeData,
    amount_msat: u64,
    amount_sent_msat: u64,
) {
    if is_self_pay {
        accumulate_msat(
            &mut full_node_data.totals.pays.self_fees_msat,
            fee_msat.unwrap(),
        );
        accumulate_msat(
            &mut full_node_data.totals.pays.self_amount_msat,
            amount_msat,
        );
        accumulate_msat(
            &mut full_node_data.totals.pays.self_amount_sent_msat,
            amount_sent_msat,
        );
    } else {
        accumulate_msat(&mut full_node_data.totals.pays.fees_msat, fee_msat.unwrap());
        accumulate_msat(&mut full_node_data.totals.pays.amount_msat, amount_msat);
        accumulate_msat(
            &mut full_node_data.totals.pays.amount_sent_msat,
            amount_sent_msat,
        );
    }
}

async fn extract_pay_descriptions(
    now: Instant,
    rpc: &mut ClnRpc,
    full_node_data: &mut FullNodeData,
) -> Result<(), Error> {
    for pay in &mut full_node_data.pays {
        match pay.description_status {
            DescriptionStatus::Processed => {}
            DescriptionStatus::Bolt11 => {
                let Ok(decoded_invoice) = Bolt11Invoice::from_str(
                    pay.description
                        .as_ref()
                        .ok_or_else(|| anyhow!("expected bolt11 string to decode"))?,
                ) else {
                    pay.description = None;
                    log::warn!("Could not decode bolt11, skipping");
                    continue;
                };
                pay.description = Some(decoded_invoice.description().to_string());
            }
            DescriptionStatus::Bolt12 => {
                pay.description = rpc
                    .call_typed(&DecodeRequest {
                        string: pay
                            .description
                            .as_ref()
                            .ok_or_else(|| anyhow!("expected bolt12 string to decode"))?
                            .to_owned(),
                    })
                    .await?
                    .offer_description;
            }
        }
    }

    log::debug!(
        "Extracted pays descriptions done. Total: {}ms",
        now.elapsed().as_millis()
    );
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

    paystable.with(
        Modify::new(ByColumnName::new(PaysColumns::completed_at.to_string()).not(Rows::first()))
            .with(Format::content(|timestamp| {
                timestamp_to_localized_datetime_string(config, timestamp.parse::<u64>().unwrap())
                    .unwrap_or("ERROR".to_owned())
            })),
    );

    paystable.with(Modify::new(Rows::first()).with(Alignment::center()));

    let mut pays_totals = String::new();

    if full_node_data.totals.pays.amount_sent_msat.is_some() {
        write!(
            pays_totals,
            "\nTotal of {} pays in the last {}h: {} sats_requested {} sats_sent {} fee_sats",
            full_node_data.totals.pays.count,
            config.pays,
            if let Some(amt) = full_node_data.totals.pays.amount_msat {
                u64_to_sat_string(config, rounded_div_u64(amt, 1_000))?
            } else {
                MISSING_VALUE.to_owned()
            },
            u64_to_sat_string(
                config,
                rounded_div_u64(full_node_data.totals.pays.amount_sent_msat.unwrap(), 1_000)
            )?,
            if let Some(fee) = full_node_data.totals.pays.fees_msat {
                u64_to_sat_string(config, rounded_div_u64(fee, 1000))?
            } else {
                MISSING_VALUE.to_owned()
            },
        )?;
    }

    if full_node_data.totals.pays.self_amount_sent_msat.is_some() {
        write!(
            pays_totals,
            "\nTotal of {} self-pays in the last {}h: {} sats_requested {} sats_sent {} fee_sats",
            full_node_data.totals.pays.self_count,
            config.pays,
            if let Some(amt) = full_node_data.totals.pays.self_amount_msat {
                u64_to_sat_string(config, rounded_div_u64(amt, 1_000))?
            } else {
                MISSING_VALUE.to_owned()
            },
            u64_to_sat_string(
                config,
                rounded_div_u64(
                    full_node_data.totals.pays.self_amount_sent_msat.unwrap(),
                    1_000
                )
            )?,
            if let Some(fee) = full_node_data.totals.pays.self_fees_msat {
                u64_to_sat_string(config, rounded_div_u64(fee, 1000))?
            } else {
                MISSING_VALUE.to_owned()
            },
        )?;
    }

    if !pays_totals.is_empty() {
        paystable.with(Panel::footer(pays_totals));
    }

    let mut result = format!(
        "\n\npays (last {}h, limit: {}):\n",
        config.pays,
        if config.pays_limit > 0 {
            format!("{}/{}", count, config.pays_limit)
        } else {
            "off".to_owned()
        }
    );
    writeln!(result, "{paystable}")?;

    Ok(result)
}
