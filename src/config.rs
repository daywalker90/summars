use std::str::FromStr;

use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::{
    options::{self},
    ConfiguredPlugin,
    Plugin,
};
use cln_rpc::RpcError;
use icu_locale::Locale;
use serde_json::json;
use strum::IntoEnumIterator;

use crate::{
    structs::{
        ChannelVisibility,
        Config,
        ConnectionStatus,
        ExcludeStates,
        ForwardsColumns,
        InvoicesColumns,
        Opt,
        PaysColumns,
        ShortChannelState,
        Styles,
        SummaryColumns,
        TableColumn,
    },
    PluginState,
};

pub async fn setconfig_callback(
    plugin: Plugin<PluginState>,
    args: serde_json::Value,
) -> Result<serde_json::Value, Error> {
    let name = args
        .get("config")
        .ok_or_else(|| anyhow!("Bad CLN object. No option name found!"))?
        .as_str()
        .ok_or_else(|| anyhow!("Bad CLN object. Option name not a string!"))?;
    let value = args
        .get("val")
        .ok_or_else(|| anyhow!("Bad CLN object. No value found for option: {name}"))?;

    let opt = Opt::from_key(name)?;

    let opt_value = parse_option(opt, value).map_err(|e| {
        anyhow!(json!(RpcError {
            code: Some(-32602),
            message: e.to_string(),
            data: None
        }))
    })?;

    let mut config = plugin.state().config.lock();

    check_option(&mut config, opt, &opt_value).map_err(|e| {
        anyhow!(json!(RpcError {
            code: Some(-32602),
            message: e.to_string(),
            data: None
        }))
    })?;

    plugin.set_option_str(name, opt_value).map_err(|e| {
        anyhow!(json!(RpcError {
            code: Some(-32602),
            message: e.to_string(),
            data: None
        }))
    })?;

    Ok(json!({}))
}

fn parse_option(opt: Opt, value: &serde_json::Value) -> Result<options::Value, Error> {
    match opt {
        Opt::Forwards
        | Opt::ForwardsLimit
        | Opt::ForwardsFilterAmt
        | Opt::ForwardsFilterFee
        | Opt::Pays
        | Opt::PaysLimit
        | Opt::MaxDescLength
        | Opt::Invoices
        | Opt::InvoicesLimit
        | Opt::MaxLabelLength
        | Opt::InvoicesFilterAmt
        | Opt::RefreshAlias
        | Opt::MaxAliasLength
        | Opt::AvailabilityInterval
        | Opt::AvailabilityWindow => {
            if let Some(n_i64) = value.as_i64() {
                return Ok(options::Value::Integer(n_i64));
            } else if let Some(n_str) = value.as_str() {
                if let Ok(n_neg_i64) = n_str.parse::<i64>() {
                    return Ok(options::Value::Integer(n_neg_i64));
                }
            }
            Err(anyhow!("{} is not a valid integer!", opt.as_key()))
        }
        Opt::Utf8 | Opt::Json => {
            if let Some(n_bool) = value.as_bool() {
                return Ok(options::Value::Boolean(n_bool));
            } else if let Some(n_str) = value.as_str() {
                if let Ok(n_str_bool) = n_str.parse::<bool>() {
                    return Ok(options::Value::Boolean(n_str_bool));
                }
            }
            Err(anyhow!("{} is not a valid boolean!", opt.as_key()))
        }
        _ => {
            if value.is_string() {
                Ok(options::Value::String(value.as_str().unwrap().to_owned()))
            } else {
                Err(anyhow!("{} is not a valid string!", opt.as_key()))
            }
        }
    }
}

fn parse_sort_input(input: &str) -> Result<(SummaryColumns, bool), Error> {
    let reverse = input.starts_with('-');
    let input_sane = if reverse {
        input[1..].to_ascii_lowercase()
    } else {
        input.to_ascii_lowercase()
    };

    if input_sane.eq("graph_sats") {
        Err(anyhow!(
            "Can not sort by `GRAPH_SATS`, use `IN_SATS`, `OUT_SATS` or `TOTAL_SATS` instead!"
        ))
    } else if let Ok(col) = SummaryColumns::parse_column(&input_sane) {
        Ok((col, reverse))
    } else {
        Err(anyhow!(
            "`{}` is invalid. Can only sort by valid columns. Must be one of: {}",
            input.to_ascii_uppercase(),
            SummaryColumns::iter()
                .filter(|t| t != &SummaryColumns::GRAPH_SATS)
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

fn validate_exclude_states_input(input: &str) -> Result<ExcludeStates, Error> {
    let cleaned_input: String = input
        .chars()
        .filter(|&c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let split_input: Vec<&str> = cleaned_input.split(',').collect();
    if split_input.contains(&"PUBLIC") && split_input.contains(&"PRIVATE") {
        return Err(anyhow!("Can only filter `PUBLIC` OR `PRIVATE`, not both."));
    }
    if split_input.contains(&"ONLINE") && split_input.contains(&"OFFLINE") {
        return Err(anyhow!("Can only filter `ONLINE` OR `OFFLINE`, not both."));
    }
    let mut parsed_input = Vec::new();
    let mut parsed_visibility = None;
    let mut parsed_connection_status = None;
    for i in &split_input {
        if let Ok(state) = ShortChannelState::from_str(i) {
            parsed_input.push(state);
        } else if i.eq(&"PUBLIC") {
            parsed_visibility = Some(ChannelVisibility::Public);
        } else if i.eq(&"PRIVATE") {
            parsed_visibility = Some(ChannelVisibility::Private);
        } else if i.eq(&"ONLINE") {
            parsed_connection_status = Some(ConnectionStatus::Online);
        } else if i.eq(&"OFFLINE") {
            parsed_connection_status = Some(ConnectionStatus::Offline);
        } else {
            return Err(anyhow!("Could not parse channel state: `{i}`"));
        }
    }
    Ok(ExcludeStates {
        channel_states: parsed_input,
        channel_visibility: parsed_visibility,
        connection_status: parsed_connection_status,
    })
}

fn validate_u64_input(
    n: u64,
    var_name: &str,
    gteq: u64,
    check_valid_time: bool,
) -> Result<u64, Error> {
    if n < gteq {
        return Err(anyhow!(
            "{var_name} must be greater than or equal to {gteq}"
        ));
    }

    if check_valid_time && !is_valid_hour_timestamp(n) {
        return Err(anyhow!(
            "{} needs to be a positive number and smaller than {}, \
            not `{}`.",
            var_name,
            Utc::now().timestamp().unsigned_abs() / 60 / 60,
            n
        ));
    }

    Ok(n)
}

#[allow(clippy::cast_sign_loss)]
fn validate_i64_as_u64_input(n: i64) -> Option<u64> {
    if n < 0 {
        None
    } else {
        Some(n as u64)
    }
}

fn validate_i64_input_absolute(n: i64, var_name: &str, gteq: i64) -> Result<i64, Error> {
    if n.abs() < gteq {
        return Err(anyhow!(
            "{var_name} must be greater than or equal to |{gteq}|"
        ));
    }

    Ok(n)
}

fn options_value_to_u64(
    name: &str,
    value: i64,
    gteq: u64,
    check_valid_time: bool,
) -> Result<u64, Error> {
    if value >= 0 {
        validate_u64_input(value.unsigned_abs(), name, gteq, check_valid_time)
    } else {
        Err(anyhow!(
            "{name} needs to be a positive number and not `{value}`."
        ))
    }
}

fn options_value_to_usize(name: &str, value: i64) -> Result<usize, Error> {
    if value >= 0 {
        Ok(usize::try_from(value)?)
    } else {
        Err(anyhow!(
            "{name} needs to be a positive number and not `{value}`."
        ))
    }
}

pub fn validateargs(args: serde_json::Value, config: &mut Config) -> Result<(), Error> {
    let serde_json::Value::Object(map) = args else {
        return Ok(());
    };

    for (key, value) in map {
        let opt = Opt::from_key(&key)?;

        if opt.is_internal() {
            return Err(anyhow!(
                "Setting {key} here does not make sense, please use a longer-lasting method!"
            ));
        }

        let parsed = parse_option(opt, &value)?;
        check_option(config, opt, &parsed)?;
    }

    check_options_dependencies(config)?;

    Ok(())
}

pub fn get_startup_options(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
    state: &PluginState,
) -> Result<(), Error> {
    {
        let mut config = state.config.lock();

        for opt in Opt::iter() {
            if let Some(opt_val) = plugin.option_str(opt.as_key())? {
                check_option(&mut config, opt, &opt_val)?;
            }
        }
    }
    Ok(())
}

fn check_option(config: &mut Config, opt: Opt, value: &options::Value) -> Result<(), Error> {
    match opt {
        Opt::Columns => {
            config.columns = SummaryColumns::parse_columns(value.as_str().unwrap())?;
        }
        Opt::SortBy => {
            (config.sort_by, config.sort_reverse) = parse_sort_input(value.as_str().unwrap())?;
        }
        Opt::ExcludeChannelStates => {
            config.exclude_channel_states = validate_exclude_states_input(value.as_str().unwrap())?;
        }
        Opt::Forwards => {
            config.forwards = options_value_to_u64(opt.as_key(), value.as_i64().unwrap(), 0, true)?;
        }
        Opt::ForwardsLimit => {
            config.forwards_limit = options_value_to_usize(opt.as_key(), value.as_i64().unwrap())?;
        }
        Opt::ForwardsColumns => {
            config.forwards_columns = ForwardsColumns::parse_columns(value.as_str().unwrap())?;
        }
        Opt::ForwardsFilterAmt => {
            config.forwards_filter_amt_msat = validate_i64_as_u64_input(value.as_i64().unwrap());
        }
        Opt::ForwardsFilterFee => {
            config.forwards_filter_fee_msat = validate_i64_as_u64_input(value.as_i64().unwrap());
        }
        Opt::Pays => {
            config.pays = options_value_to_u64(opt.as_key(), value.as_i64().unwrap(), 0, true)?;
        }
        Opt::PaysLimit => {
            config.pays_limit = options_value_to_usize(opt.as_key(), value.as_i64().unwrap())?;
        }
        Opt::PaysColumns => {
            config.pays_columns = PaysColumns::parse_columns(value.as_str().unwrap())?;
        }
        Opt::MaxDescLength => {
            config.max_desc_length =
                validate_i64_input_absolute(value.as_i64().unwrap(), opt.as_key(), 5)?;
        }
        Opt::Invoices => {
            config.invoices = options_value_to_u64(opt.as_key(), value.as_i64().unwrap(), 0, true)?;
        }
        Opt::InvoicesLimit => {
            config.invoices_limit = options_value_to_usize(opt.as_key(), value.as_i64().unwrap())?;
        }
        Opt::InvoicesColumns => {
            config.invoices_columns = InvoicesColumns::parse_columns(value.as_str().unwrap())?;
        }
        Opt::MaxLabelLength => {
            config.max_label_length =
                validate_i64_input_absolute(value.as_i64().unwrap(), opt.as_key(), 5)?;
        }
        Opt::InvoicesFilterAmt => {
            config.invoices_filter_amt_msat = validate_i64_as_u64_input(value.as_i64().unwrap());
        }
        Opt::Locale => {
            config.locale = match Locale::from_str(value.as_str().unwrap()) {
                Ok(l) => l,
                Err(e) => {
                    return Err(anyhow!(
                        "`{}` is not a valid locale: {}",
                        value.as_str().unwrap(),
                        e
                    ))
                }
            }
        }
        Opt::RefreshAlias => {
            config.refresh_alias =
                options_value_to_u64(opt.as_key(), value.as_i64().unwrap(), 1, false)?;
        }
        Opt::MaxAliasLength => {
            config.max_alias_length =
                validate_i64_input_absolute(value.as_i64().unwrap(), opt.as_key(), 5)?;
        }
        Opt::AvailabilityInterval => {
            config.availability_interval =
                options_value_to_u64(opt.as_key(), value.as_i64().unwrap(), 1, false)?;
        }
        Opt::AvailabilityWindow => {
            config.availability_window =
                options_value_to_u64(opt.as_key(), value.as_i64().unwrap(), 1, false)?;
        }
        Opt::Utf8 => config.utf8 = value.as_bool().unwrap(),
        Opt::Style => config.style = Styles::from_str(value.as_str().unwrap())?,
        Opt::FlowStyle => config.flow_style = Styles::from_str(value.as_str().unwrap())?,
        Opt::Json => config.json = value.as_bool().unwrap(),
        // _ => return Err(anyhow!("Unknown option: {name}")),
    }
    Ok(())
}

fn check_options_dependencies(config: &Config) -> Result<(), Error> {
    if config.forwards_limit > 0 && config.forwards == 0 {
        return Err(anyhow!(
            "You must set `{}` for `{}` to have an effect!",
            Opt::Forwards.as_key(),
            Opt::ForwardsLimit.as_key()
        ));
    }
    if config.pays_limit > 0 && config.pays == 0 {
        return Err(anyhow!(
            "You must set `{}` for `{}` to have an effect!",
            Opt::Pays.as_key(),
            Opt::PaysLimit.as_key()
        ));
    }
    if config.invoices_limit > 0 && config.invoices == 0 {
        return Err(anyhow!(
            "You must set `{}` for `{}` to have an effect!",
            Opt::Invoices.as_key(),
            Opt::InvoicesLimit.as_key()
        ));
    }
    Ok(())
}

fn is_valid_hour_timestamp(val: u64) -> bool {
    Utc::now().timestamp().unsigned_abs() > val * 60 * 60
}
