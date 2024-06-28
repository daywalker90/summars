use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::{
    options::{self},
    ConfiguredPlugin, Plugin,
};
use cln_rpc::RpcError;
use icu_locid::Locale;
use serde_json::json;
use std::{collections::HashSet, str::FromStr};
use struct_field_names_as_array::FieldNamesAsArray;

use crate::{
    structs::{
        ChannelVisibility, Config, ConnectionStatus, ExcludeStates, Forwards, Invoices, Pays,
        ShortChannelState, Styles, Summary,
    },
    PluginState, OPT_AVAILABILITY_INTERVAL, OPT_AVAILABILITY_WINDOW, OPT_COLUMNS,
    OPT_EXCLUDE_CHANNEL_STATES, OPT_FLOW_STYLE, OPT_FORWARDS, OPT_FORWARDS_ALIAS,
    OPT_FORWARDS_COLUMNS, OPT_FORWARDS_FILTER_AMT, OPT_FORWARDS_FILTER_FEE, OPT_INVOICES,
    OPT_INVOICES_COLUMNS, OPT_INVOICES_FILTER_AMT, OPT_JSON, OPT_LOCALE, OPT_MAX_ALIAS_LENGTH,
    OPT_MAX_DESC_LENGTH, OPT_MAX_LABEL_LENGTH, OPT_PAYS, OPT_PAYS_COLUMNS, OPT_REFRESH_ALIAS,
    OPT_SORT_BY, OPT_STYLE, OPT_UTF8,
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

    let opt_value = parse_option(name, value).map_err(|e| {
        anyhow!(json!(RpcError {
            code: Some(-32602),
            message: e.to_string(),
            data: None
        }))
    })?;

    let mut config = plugin.state().config.lock();

    check_option(&mut config, name, &opt_value).map_err(|e| {
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

fn parse_option(name: &str, value: &serde_json::Value) -> Result<options::Value, Error> {
    match name {
        n if n.eq(OPT_FORWARDS)
            || n.eq(OPT_FORWARDS_FILTER_AMT)
            || n.eq(OPT_FORWARDS_FILTER_FEE)
            || n.eq(OPT_PAYS)
            || n.eq(OPT_MAX_DESC_LENGTH)
            || n.eq(OPT_INVOICES)
            || n.eq(OPT_MAX_LABEL_LENGTH)
            || n.eq(OPT_INVOICES_FILTER_AMT)
            || n.eq(OPT_REFRESH_ALIAS)
            || n.eq(OPT_MAX_ALIAS_LENGTH)
            || n.eq(OPT_AVAILABILITY_INTERVAL)
            || n.eq(OPT_AVAILABILITY_WINDOW) =>
        {
            if let Some(n_i64) = value.as_i64() {
                return Ok(options::Value::Integer(n_i64));
            } else if let Some(n_str) = value.as_str() {
                if let Ok(n_neg_i64) = n_str.parse::<i64>() {
                    return Ok(options::Value::Integer(n_neg_i64));
                }
            }
            Err(anyhow!("{} is not a valid integer!", n))
        }
        n if n.eq(OPT_FORWARDS_ALIAS) || n.eq(OPT_UTF8) || n.eq(OPT_JSON) => {
            if let Some(n_bool) = value.as_bool() {
                return Ok(options::Value::Boolean(n_bool));
            } else if let Some(n_str) = value.as_str() {
                if let Ok(n_str_bool) = n_str.parse::<bool>() {
                    return Ok(options::Value::Boolean(n_str_bool));
                }
            }
            Err(anyhow!("{} is not a valid boolean!", n))
        }
        _ => {
            if value.is_string() {
                Ok(options::Value::String(value.as_str().unwrap().to_owned()))
            } else {
                Err(anyhow!("{} is not a valid string!", name))
            }
        }
    }
}

fn validate_columns_input(input: &str) -> Result<Vec<String>, Error> {
    let cleaned_input: String = input
        .chars()
        .filter(|&c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let split_input: Vec<&str> = cleaned_input.split(',').collect();

    let mut uniq = HashSet::new();
    for i in &split_input {
        if !uniq.insert(i) {
            return Err(anyhow!(
                "Duplicate entry detected in {}: {}",
                OPT_COLUMNS,
                i
            ));
        }
    }

    for i in &split_input {
        if !Summary::FIELD_NAMES_AS_ARRAY.contains(i) {
            return Err(anyhow!("`{}` not found in valid column names!", i));
        }
    }

    let cleaned_strings: Vec<String> = split_input.into_iter().map(String::from).collect();
    Ok(cleaned_strings)
}

fn validate_forwards_columns_input(input: &str) -> Result<Vec<String>, Error> {
    let cleaned_input: String = input
        .chars()
        .filter(|&c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let split_input: Vec<&str> = cleaned_input.split(',').collect();

    let mut uniq = HashSet::new();
    for i in &split_input {
        if !uniq.insert(i) {
            return Err(anyhow!(
                "Duplicate entry detected in {}: {}",
                OPT_FORWARDS_COLUMNS,
                i
            ));
        }
    }

    for i in &split_input {
        if !Forwards::FIELD_NAMES_AS_ARRAY.contains(i) {
            return Err(anyhow!("`{}` not found in valid forwards column names!", i));
        }
    }

    let cleaned_strings: Vec<String> = split_input.into_iter().map(String::from).collect();
    Ok(cleaned_strings)
}

fn validate_pays_columns_input(input: &str) -> Result<Vec<String>, Error> {
    let cleaned_input: String = input
        .chars()
        .filter(|&c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let split_input: Vec<&str> = cleaned_input.split(',').collect();

    let mut uniq = HashSet::new();
    for i in &split_input {
        if !uniq.insert(i) {
            return Err(anyhow!(
                "Duplicate entry detected in {}: {}",
                OPT_PAYS_COLUMNS,
                i
            ));
        }
    }

    for i in &split_input {
        if !Pays::FIELD_NAMES_AS_ARRAY.contains(i) {
            return Err(anyhow!("`{}` not found in valid pays column names!", i));
        }
    }

    let cleaned_strings: Vec<String> = split_input.into_iter().map(String::from).collect();
    Ok(cleaned_strings)
}

fn validate_invoices_columns_input(input: &str) -> Result<Vec<String>, Error> {
    let cleaned_input: String = input
        .chars()
        .filter(|&c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let split_input: Vec<&str> = cleaned_input.split(',').collect();

    let mut uniq = HashSet::new();
    for i in &split_input {
        if !uniq.insert(i) {
            return Err(anyhow!(
                "Duplicate entry detected in {}: {}",
                OPT_INVOICES_COLUMNS,
                i
            ));
        }
    }

    for i in &split_input {
        if !Invoices::FIELD_NAMES_AS_ARRAY.contains(i) {
            return Err(anyhow!("`{}` not found in valid invoices column names!", i));
        }
    }

    let cleaned_strings: Vec<String> = split_input.into_iter().map(String::from).collect();
    Ok(cleaned_strings)
}

fn validate_sort_input(input: &str) -> Result<String, Error> {
    let reverse = input.starts_with('-');

    let sortable_columns = Summary::FIELD_NAMES_AS_ARRAY
        .into_iter()
        .filter(|t| t != &"GRAPH_SATS")
        .collect::<Vec<&str>>();

    if reverse && sortable_columns.contains(&&input[1..]) || sortable_columns.contains(&input) {
        Ok(input.to_string())
    } else {
        Err(anyhow!(
            "Not a valid column name: `{}`. Must be one of: {}",
            input,
            sortable_columns.join(", ")
        ))
    }
}

fn validate_exclude_states_input(input: &str) -> Result<ExcludeStates, Error> {
    let cleaned_input: String = input.chars().filter(|&c| !c.is_whitespace()).collect();
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
            parsed_visibility = Some(ChannelVisibility::Public)
        } else if i.eq(&"PRIVATE") {
            parsed_visibility = Some(ChannelVisibility::Private)
        } else if i.eq(&"ONLINE") {
            parsed_connection_status = Some(ConnectionStatus::Online)
        } else if i.eq(&"OFFLINE") {
            parsed_connection_status = Some(ConnectionStatus::Offline)
        } else {
            return Err(anyhow!("Could not parse channel state: `{}`", i));
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
            "{} must be greater than or equal to {}",
            var_name,
            gteq
        ));
    }

    if check_valid_time && !is_valid_hour_timestamp(n) {
        return Err(anyhow!(
            "{} needs to be a positive number and smaller than {}, \
            not `{}`.",
            var_name,
            (Utc::now().timestamp() as u64) / 60 / 60,
            n
        ));
    }

    Ok(n)
}

fn validate_i64_input(n: i64, var_name: &str, gteq: i64) -> Result<i64, Error> {
    if n < gteq {
        return Err(anyhow!(
            "{} must be greater than or equal to {}",
            var_name,
            gteq
        ));
    }

    Ok(n)
}

fn validate_i64_input_absolute(n: i64, var_name: &str, gteq: i64) -> Result<i64, Error> {
    if n.abs() < gteq {
        return Err(anyhow!(
            "{} must be greater than or equal to |{}|",
            var_name,
            gteq
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
        validate_u64_input(value as u64, name, gteq, check_valid_time)
    } else {
        Err(anyhow!(
            "{} needs to be a positive number and not `{}`.",
            name,
            value
        ))
    }
}

pub fn validateargs(args: serde_json::Value, config: &mut Config) -> Result<(), Error> {
    if let serde_json::Value::Object(i) = args {
        for (key, value) in i.iter() {
            match key {
                name if name.eq(OPT_COLUMNS) => {
                    check_option(config, OPT_COLUMNS, &parse_option(OPT_COLUMNS, value)?)?
                }
                name if name.eq(OPT_SORT_BY) => {
                    check_option(config, OPT_SORT_BY, &parse_option(OPT_SORT_BY, value)?)?
                }
                name if name.eq(OPT_EXCLUDE_CHANNEL_STATES) => check_option(
                    config,
                    OPT_EXCLUDE_CHANNEL_STATES,
                    &parse_option(OPT_EXCLUDE_CHANNEL_STATES, value)?,
                )?,
                name if name.eq(OPT_FORWARDS) => {
                    check_option(config, OPT_FORWARDS, &parse_option(OPT_FORWARDS, value)?)?
                }
                name if name.eq(OPT_FORWARDS_COLUMNS) => check_option(
                    config,
                    OPT_FORWARDS_COLUMNS,
                    &parse_option(OPT_FORWARDS_COLUMNS, value)?,
                )?,
                name if name.eq(OPT_FORWARDS_FILTER_AMT) => check_option(
                    config,
                    OPT_FORWARDS_FILTER_AMT,
                    &parse_option(OPT_FORWARDS_FILTER_AMT, value)?,
                )?,
                name if name.eq(OPT_FORWARDS_FILTER_FEE) => check_option(
                    config,
                    OPT_FORWARDS_FILTER_FEE,
                    &parse_option(OPT_FORWARDS_FILTER_FEE, value)?,
                )?,
                name if name.eq(OPT_FORWARDS_ALIAS) => check_option(
                    config,
                    OPT_FORWARDS_ALIAS,
                    &parse_option(OPT_FORWARDS_ALIAS, value)?,
                )?,
                name if name.eq(OPT_PAYS) => {
                    check_option(config, OPT_PAYS, &parse_option(OPT_PAYS, value)?)?
                }
                name if name.eq(OPT_PAYS_COLUMNS) => check_option(
                    config,
                    OPT_PAYS_COLUMNS,
                    &parse_option(OPT_PAYS_COLUMNS, value)?,
                )?,
                name if name.eq(OPT_MAX_DESC_LENGTH) => check_option(
                    config,
                    OPT_MAX_DESC_LENGTH,
                    &parse_option(OPT_MAX_DESC_LENGTH, value)?,
                )?,
                name if name.eq(OPT_INVOICES) => {
                    check_option(config, OPT_INVOICES, &parse_option(OPT_INVOICES, value)?)?
                }
                name if name.eq(OPT_INVOICES_COLUMNS) => check_option(
                    config,
                    OPT_INVOICES_COLUMNS,
                    &parse_option(OPT_INVOICES_COLUMNS, value)?,
                )?,
                name if name.eq(OPT_MAX_LABEL_LENGTH) => check_option(
                    config,
                    OPT_MAX_LABEL_LENGTH,
                    &parse_option(OPT_MAX_LABEL_LENGTH, value)?,
                )?,
                name if name.eq(OPT_INVOICES_FILTER_AMT) => check_option(
                    config,
                    OPT_INVOICES_FILTER_AMT,
                    &parse_option(OPT_INVOICES_FILTER_AMT, value)?,
                )?,
                name if name.eq(OPT_LOCALE) => {
                    check_option(config, OPT_LOCALE, &parse_option(OPT_LOCALE, value)?)?
                }
                name if name.eq(OPT_MAX_ALIAS_LENGTH) => check_option(
                    config,
                    OPT_MAX_ALIAS_LENGTH,
                    &parse_option(OPT_MAX_ALIAS_LENGTH, value)?,
                )?,
                name if name.eq(OPT_UTF8) => {
                    check_option(config, OPT_UTF8, &parse_option(OPT_UTF8, value)?)?
                }
                name if name.eq(OPT_STYLE) => {
                    check_option(config, OPT_STYLE, &parse_option(OPT_STYLE, value)?)?
                }
                name if name.eq(OPT_FLOW_STYLE) => check_option(
                    config,
                    OPT_FLOW_STYLE,
                    &parse_option(OPT_FLOW_STYLE, value)?,
                )?,
                name if name.eq(OPT_JSON) => {
                    check_option(config, OPT_JSON, &parse_option(OPT_JSON, value)?)?
                }
                name if name.eq(OPT_REFRESH_ALIAS)
                    || name.eq(OPT_AVAILABILITY_INTERVAL)
                    || name.eq(OPT_AVAILABILITY_WINDOW) =>
                {
                    return Err(anyhow!(
                        "Setting {name} here does \
                not make sense, please use one of the more longer lasting methods!"
                    ))
                }
                other => return Err(anyhow!("option not found:{}", other)),
            };
        }
    };
    Ok(())
}

pub fn get_startup_options(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
    state: PluginState,
) -> Result<(), Error> {
    {
        let mut config = state.config.lock();

        if let Some(cols) = plugin.option_str(OPT_COLUMNS)? {
            check_option(&mut config, OPT_COLUMNS, &cols)?;
        };
        if let Some(sort_by) = plugin.option_str(OPT_SORT_BY)? {
            check_option(&mut config, OPT_SORT_BY, &sort_by)?;
        };
        if let Some(states) = plugin.option_str(OPT_EXCLUDE_CHANNEL_STATES)? {
            check_option(&mut config, OPT_EXCLUDE_CHANNEL_STATES, &states)?;
        };
        if let Some(fws) = plugin.option_str(OPT_FORWARDS)? {
            check_option(&mut config, OPT_FORWARDS, &fws)?;
        };
        if let Some(cols) = plugin.option_str(OPT_FORWARDS_COLUMNS)? {
            check_option(&mut config, OPT_FORWARDS_COLUMNS, &cols)?;
        };
        if let Some(ffa) = plugin.option_str(OPT_FORWARDS_FILTER_AMT)? {
            check_option(&mut config, OPT_FORWARDS_FILTER_AMT, &ffa)?;
        };
        if let Some(fff) = plugin.option_str(OPT_FORWARDS_FILTER_FEE)? {
            check_option(&mut config, OPT_FORWARDS_FILTER_FEE, &fff)?;
        };
        if let Some(fa) = plugin.option_str(OPT_FORWARDS_ALIAS)? {
            check_option(&mut config, OPT_FORWARDS_ALIAS, &fa)?;
        };
        if let Some(pays) = plugin.option_str(OPT_PAYS)? {
            check_option(&mut config, OPT_PAYS, &pays)?;
        };
        if let Some(cols) = plugin.option_str(OPT_PAYS_COLUMNS)? {
            check_option(&mut config, OPT_PAYS_COLUMNS, &cols)?;
        };
        if let Some(mdl) = plugin.option_str(OPT_MAX_DESC_LENGTH)? {
            check_option(&mut config, OPT_MAX_DESC_LENGTH, &mdl)?;
        };
        if let Some(invs) = plugin.option_str(OPT_INVOICES)? {
            check_option(&mut config, OPT_INVOICES, &invs)?;
        };
        if let Some(cols) = plugin.option_str(OPT_INVOICES_COLUMNS)? {
            check_option(&mut config, OPT_INVOICES_COLUMNS, &cols)?;
        };
        if let Some(mll) = plugin.option_str(OPT_MAX_LABEL_LENGTH)? {
            check_option(&mut config, OPT_MAX_LABEL_LENGTH, &mll)?;
        };
        if let Some(invfa) = plugin.option_str(OPT_INVOICES_FILTER_AMT)? {
            check_option(&mut config, OPT_INVOICES_FILTER_AMT, &invfa)?;
        };
        if let Some(loc) = plugin.option_str(OPT_LOCALE)? {
            check_option(&mut config, OPT_LOCALE, &loc)?;
        };
        if let Some(ra) = plugin.option_str(OPT_REFRESH_ALIAS)? {
            check_option(&mut config, OPT_REFRESH_ALIAS, &ra)?;
        };
        if let Some(mal) = plugin.option_str(OPT_MAX_ALIAS_LENGTH)? {
            check_option(&mut config, OPT_MAX_ALIAS_LENGTH, &mal)?;
        };
        if let Some(ai) = plugin.option_str(OPT_AVAILABILITY_INTERVAL)? {
            check_option(&mut config, OPT_AVAILABILITY_INTERVAL, &ai)?;
        };
        if let Some(aw) = plugin.option_str(OPT_AVAILABILITY_WINDOW)? {
            check_option(&mut config, OPT_AVAILABILITY_WINDOW, &aw)?;
        };
        if let Some(utf8) = plugin.option_str(OPT_UTF8)? {
            check_option(&mut config, OPT_UTF8, &utf8)?;
        };
        if let Some(style) = plugin.option_str(OPT_STYLE)? {
            check_option(&mut config, OPT_STYLE, &style)?;
        };
        if let Some(fstyle) = plugin.option_str(OPT_FLOW_STYLE)? {
            check_option(&mut config, OPT_FLOW_STYLE, &fstyle)?;
        };
        if let Some(js) = plugin.option_str(OPT_JSON)? {
            check_option(&mut config, OPT_JSON, &js)?;
        };
    }
    Ok(())
}

fn check_option(config: &mut Config, name: &str, value: &options::Value) -> Result<(), Error> {
    match name {
        n if n.eq(OPT_COLUMNS) => {
            config.columns.value = validate_columns_input(value.as_str().unwrap())?;
        }
        n if n.eq(OPT_SORT_BY) => {
            config.sort_by.value = validate_sort_input(value.as_str().unwrap())?
        }
        n if n.eq(OPT_EXCLUDE_CHANNEL_STATES) => {
            config.exclude_channel_states.value =
                validate_exclude_states_input(value.as_str().unwrap())?;
        }
        n if n.eq(OPT_FORWARDS) => {
            config.forwards.value =
                options_value_to_u64(OPT_FORWARDS, value.as_i64().unwrap(), 0, true)?;
        }
        n if n.eq(OPT_FORWARDS_COLUMNS) => {
            config.forwards_columns.value =
                validate_forwards_columns_input(value.as_str().unwrap())?;
        }
        n if n.eq(OPT_FORWARDS_FILTER_AMT) => {
            config.forwards_filter_amt_msat.value =
                validate_i64_input(value.as_i64().unwrap(), OPT_FORWARDS_FILTER_AMT, -1)?;
        }
        n if n.eq(OPT_FORWARDS_FILTER_FEE) => {
            config.forwards_filter_fee_msat.value =
                validate_i64_input(value.as_i64().unwrap(), OPT_FORWARDS_FILTER_FEE, -1)?;
        }
        n if n.eq(OPT_FORWARDS_ALIAS) => {
            config.forwards_alias.value = value.as_bool().unwrap();
        }
        n if n.eq(OPT_PAYS) => {
            config.pays.value = options_value_to_u64(OPT_PAYS, value.as_i64().unwrap(), 0, true)?
        }
        n if n.eq(OPT_PAYS_COLUMNS) => {
            config.pays_columns.value = validate_pays_columns_input(value.as_str().unwrap())?;
        }
        n if n.eq(OPT_MAX_DESC_LENGTH) => {
            config.max_desc_length.value =
                validate_i64_input_absolute(value.as_i64().unwrap(), OPT_MAX_DESC_LENGTH, 5)?
        }
        n if n.eq(OPT_INVOICES) => {
            config.invoices.value =
                options_value_to_u64(OPT_INVOICES, value.as_i64().unwrap(), 0, true)?
        }
        n if n.eq(OPT_INVOICES_COLUMNS) => {
            config.invoices_columns.value =
                validate_invoices_columns_input(value.as_str().unwrap())?;
        }
        n if n.eq(OPT_MAX_LABEL_LENGTH) => {
            config.max_label_length.value =
                validate_i64_input_absolute(value.as_i64().unwrap(), OPT_MAX_LABEL_LENGTH, 5)?
        }
        n if n.eq(OPT_INVOICES_FILTER_AMT) => {
            config.invoices_filter_amt_msat.value =
                validate_i64_input(value.as_i64().unwrap(), OPT_INVOICES_FILTER_AMT, -1)?
        }
        n if n.eq(OPT_LOCALE) => {
            config.locale.value = match Locale::from_str(value.as_str().unwrap()) {
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
        n if n.eq(OPT_REFRESH_ALIAS) => {
            config.refresh_alias.value =
                options_value_to_u64(OPT_REFRESH_ALIAS, value.as_i64().unwrap(), 1, false)?
        }
        n if n.eq(OPT_MAX_ALIAS_LENGTH) => {
            config.max_alias_length.value =
                validate_i64_input_absolute(value.as_i64().unwrap(), OPT_MAX_ALIAS_LENGTH, 5)?
        }
        n if n.eq(OPT_AVAILABILITY_INTERVAL) => {
            config.availability_interval.value =
                options_value_to_u64(OPT_AVAILABILITY_INTERVAL, value.as_i64().unwrap(), 1, false)?
        }
        n if n.eq(OPT_AVAILABILITY_WINDOW) => {
            config.availability_window.value =
                options_value_to_u64(OPT_AVAILABILITY_WINDOW, value.as_i64().unwrap(), 1, false)?
        }
        n if n.eq(OPT_UTF8) => config.utf8.value = value.as_bool().unwrap(),
        n if n.eq(OPT_STYLE) => config.style.value = Styles::from_str(value.as_str().unwrap())?,
        n if n.eq(OPT_FLOW_STYLE) => {
            config.flow_style.value = Styles::from_str(value.as_str().unwrap())?
        }
        n if n.eq(OPT_JSON) => config.json.value = value.as_bool().unwrap(),
        _ => return Err(anyhow!("Unknown option: {}", name)),
    }
    Ok(())
}

fn is_valid_hour_timestamp(val: u64) -> bool {
    Utc::now().timestamp() as u64 > val * 60 * 60
}
