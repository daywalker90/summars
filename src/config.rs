use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::{
    options::{config_type::Integer, ConfigOption},
    ConfiguredPlugin,
};
use icu_locid::Locale;
use log::warn;
use parking_lot::Mutex;
use std::path::Path;
use std::{str::FromStr, sync::Arc};
use struct_field_names_as_array::FieldNamesAsArray;
use tokio::fs;

use crate::{
    structs::{ChannelVisibility, Config, ShortChannelState, Styles, Summary},
    PluginState, OPT_AVAILABILITY_INTERVAL, OPT_AVAILABILITY_WINDOW, OPT_COLUMNS,
    OPT_EXCLUDE_CHANNEL_STATES, OPT_FLOW_STYLE, OPT_FORWARDS, OPT_FORWARDS_ALIAS,
    OPT_FORWARDS_FILTER_AMT, OPT_FORWARDS_FILTER_FEE, OPT_INVOICES, OPT_INVOICES_FILTER_AMT,
    OPT_JSON, OPT_LOCALE, OPT_MAX_ALIAS_LENGTH, OPT_PAYS, OPT_REFRESH_ALIAS, OPT_SORT_BY,
    OPT_STYLE, OPT_UTF8,
};

fn validate_columns_input(input: &str) -> Result<Vec<String>, Error> {
    let cleaned_input: String = input.chars().filter(|&c| !c.is_whitespace()).collect();
    let split_input: Vec<&str> = cleaned_input.split(',').collect();

    for i in &split_input {
        if !Summary::FIELD_NAMES_AS_ARRAY.contains(i) {
            return Err(anyhow!("`{}` not found in valid column names!", i));
        }
    }

    let cleaned_strings: Vec<String> = split_input.into_iter().map(String::from).collect();
    Ok(cleaned_strings)
}

fn validate_sort_input(input: &str) -> Result<String, Error> {
    let reverse = input.starts_with('-');

    if reverse && Summary::FIELD_NAMES_AS_ARRAY.contains(&&input[1..])
        || Summary::FIELD_NAMES_AS_ARRAY.contains(&input)
    {
        Ok(input.to_string())
    } else {
        Err(anyhow!(
            "Not a valid column name: `{}`. Must be one of: {}",
            input,
            Summary::field_names_to_string()
        ))
    }
}

fn validate_exclude_states_input(
    input: &str,
) -> Result<(Vec<ShortChannelState>, Option<ChannelVisibility>), Error> {
    let cleaned_input: String = input.chars().filter(|&c| !c.is_whitespace()).collect();
    let split_input: Vec<&str> = cleaned_input.split(',').collect();
    if split_input.contains(&"PUBLIC") && split_input.contains(&"PRIVATE") {
        return Err(anyhow!("Can only filter `PUBLIC` OR `PRIVATE`, not both."));
    }
    let mut parsed_input = Vec::new();
    let mut parsed_visibility = None;
    for i in &split_input {
        if let Ok(state) = ShortChannelState::from_str(i) {
            parsed_input.push(state);
        } else if i.eq(&"PUBLIC") {
            parsed_visibility = Some(ChannelVisibility::Public)
        } else if i.eq(&"PRIVATE") {
            parsed_visibility = Some(ChannelVisibility::Private)
        } else {
            return Err(anyhow!("Could not parse channel state: `{}`", i));
        }
    }
    Ok((parsed_input, parsed_visibility))
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

fn options_value_to_u64(
    opt: &ConfigOption<Integer>,
    value: i64,
    gteq: u64,
    check_valid_time: bool,
) -> Result<u64, Error> {
    if value >= 0 {
        validate_u64_input(value as u64, opt.name, gteq, check_valid_time)
    } else {
        Err(anyhow!(
            "{} needs to be a positive number and not `{}`.",
            opt.name,
            value
        ))
    }
}

fn value_to_u64(
    var_name: &str,
    value: &serde_json::Value,
    gteq: u64,
    check_valid_time: bool,
) -> Result<u64, Error> {
    match value {
        serde_json::Value::Number(b) => match b.as_u64() {
            Some(n) => validate_u64_input(n, var_name, gteq, check_valid_time),
            None => Err(anyhow!(
                "Could not read a positive number for {}.",
                var_name
            )),
        },
        _ => Err(anyhow!("{} must be a positive number.", var_name)),
    }
}

fn value_to_i64(var_name: &str, value: &serde_json::Value, gteq: i64) -> Result<i64, Error> {
    match value {
        serde_json::Value::Number(b) => match b.as_i64() {
            Some(n) => validate_i64_input(n, var_name, gteq),
            None => Err(anyhow!("Could not read a number for {}.", var_name)),
        },
        _ => Err(anyhow!("{} must be a number.", var_name)),
    }
}

fn str_to_u64(
    var_name: &str,
    value: &str,
    gteq: u64,
    check_valid_time: bool,
) -> Result<u64, Error> {
    match value.parse::<u64>() {
        Ok(n) => validate_u64_input(n, var_name, gteq, check_valid_time),
        Err(e) => Err(anyhow!(
            "Could not parse a positive number from `{}` for {}: {}",
            value,
            var_name,
            e
        )),
    }
}

fn str_to_i64(var_name: &str, value: &str, gteq: i64) -> Result<i64, Error> {
    match value.parse::<i64>() {
        Ok(n) => validate_i64_input(n, var_name, gteq),
        Err(e) => Err(anyhow!(
            "Could not parse a number from `{}` for {}: {}",
            value,
            var_name,
            e
        )),
    }
}

pub fn validateargs(args: serde_json::Value, config: &mut Config) -> Result<(), Error> {
    if let serde_json::Value::Object(i) = args {
        for (key, value) in i.iter() {
            match key {
                name if name.eq(&config.columns.name) => match value {
                    serde_json::Value::String(b) => {
                        config.columns.value = validate_columns_input(b)?;
                    }
                    _ => {
                        return Err(anyhow!(
                            "Not a string. {} must be a comma separated string of: {}",
                            config.columns.name,
                            Summary::field_names_to_string()
                        ))
                    }
                },
                name if name.eq(&config.sort_by.name) => match value {
                    serde_json::Value::String(b) => config.sort_by.value = validate_sort_input(b)?,
                    _ => {
                        return Err(anyhow!(
                            "Not a string. {} must be one of: {}",
                            config.sort_by.name,
                            Summary::field_names_to_string()
                        ))
                    }
                },
                name if name.eq(&config.exclude_channel_states.name) => match value {
                    serde_json::Value::String(b) => {
                        let result = validate_exclude_states_input(b)?;
                        config.exclude_channel_states.value = result.0;
                        config.exclude_pub_priv_states = result.1;
                    }
                    _ => {
                        return Err(anyhow!(
                        "Not a string. {} must be a comma separated string of available states.",
                        config.exclude_channel_states.name
                    ))
                    }
                },
                name if name.eq(&config.forwards.name) => {
                    config.forwards.value = value_to_u64(config.forwards.name, value, 0, true)?
                }
                name if name.eq(&config.forwards_filter_amt_msat.name) => {
                    config.forwards_filter_amt_msat.value =
                        value_to_i64(config.forwards_filter_amt_msat.name, value, -1)?
                }
                name if name.eq(&config.forwards_filter_fee_msat.name) => {
                    config.forwards_filter_fee_msat.value =
                        value_to_i64(config.forwards_filter_fee_msat.name, value, -1)?
                }
                name if name.eq(&config.forwards_alias.name) => match value {
                    serde_json::Value::Bool(b) => config.forwards_alias.value = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.forwards_alias.name
                        ))
                    }
                },
                name if name.eq(&config.pays.name) => {
                    config.pays.value = value_to_u64(config.pays.name, value, 0, true)?
                }
                name if name.eq(&config.invoices.name) => {
                    config.invoices.value = value_to_u64(config.invoices.name, value, 0, true)?
                }
                name if name.eq(&config.invoices_filter_amt_msat.name) => {
                    config.invoices_filter_amt_msat.value =
                        value_to_i64(config.invoices_filter_amt_msat.name, value, -1)?
                }
                name if name.eq(&config.locale.name) => match value {
                    serde_json::Value::String(s) => {
                        config.locale.value = match Locale::from_str(s) {
                            Ok(l) => l,
                            Err(e) => return Err(anyhow!("Not a valid locale: {}. {}", s, e)),
                        }
                    }
                    _ => return Err(anyhow!("Not a valid string for: {}", config.locale.name)),
                },
                name if name.eq(&config.refresh_alias.name) => {
                    config.refresh_alias.value =
                        value_to_u64(config.refresh_alias.name, value, 1, false)?
                }
                name if name.eq(&config.max_alias_length.name) => {
                    config.max_alias_length.value =
                        value_to_u64(config.max_alias_length.name, value, 5, false)?
                }
                name if name.eq(&config.availability_interval.name) => {
                    config.availability_interval.value =
                        value_to_u64(config.availability_interval.name, value, 1, false)?
                }
                name if name.eq(&config.availability_window.name) => {
                    config.availability_window.value =
                        value_to_u64(config.availability_window.name, value, 1, false)?
                }
                name if name.eq(&config.utf8.name) => match value {
                    serde_json::Value::Bool(b) => config.utf8.value = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.utf8.name
                        ))
                    }
                },
                name if name.eq(&config.style.name) => match value {
                    serde_json::Value::String(s) => {
                        config.style.value = Styles::from_str(s)?;
                    }
                    _ => return Err(anyhow!("Not a valid string for: {}", config.style.name)),
                },
                name if name.eq(&config.flow_style.name) => match value {
                    serde_json::Value::String(s) => {
                        config.flow_style.value = Styles::from_str(s)?;
                    }
                    _ => {
                        return Err(anyhow!(
                            "Not a valid string for: {}",
                            config.flow_style.name
                        ))
                    }
                },
                name if name.eq(&config.json.name) => match value {
                    serde_json::Value::Bool(b) => config.json.value = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.json.name
                        ))
                    }
                },
                other => return Err(anyhow!("option not found:{:?}", other)),
            };
        }
    };
    Ok(())
}

pub async fn read_config(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
    state: PluginState,
) -> Result<(), Error> {
    let dir = plugin.configuration().clone().lightning_dir;
    let general_configfile =
        match fs::read_to_string(Path::new(&dir).parent().unwrap().join("config")).await {
            Ok(file2) => file2,
            Err(_) => {
                warn!("No general config file found!");
                String::new()
            }
        };
    let network_configfile = match fs::read_to_string(Path::new(&dir).join("config")).await {
        Ok(file) => file,
        Err(_) => {
            warn!("No network config file found!");
            String::new()
        }
    };

    parse_config_file(general_configfile, state.config.clone())?;
    parse_config_file(network_configfile, state.config.clone())?;
    Ok(())
}

fn parse_config_file(configfile: String, config: Arc<Mutex<Config>>) -> Result<(), Error> {
    let mut config = config.lock();
    for line in configfile.lines() {
        if line.contains('=') {
            let splitline = line.split('=').collect::<Vec<&str>>();
            if splitline.len() == 2 {
                let name = splitline.first().unwrap();
                let value = splitline.get(1).unwrap();

                match name {
                    opt if opt.eq(&config.columns.name) => {
                        config.columns.value = validate_columns_input(value)?
                    }
                    opt if opt.eq(&config.sort_by.name) => {
                        config.sort_by.value = validate_sort_input(value)?
                    }
                    opt if opt.eq(&config.exclude_channel_states.name) => {
                        let result = validate_exclude_states_input(value)?;
                        config.exclude_channel_states.value = result.0;
                        config.exclude_pub_priv_states = result.1;
                    }
                    opt if opt.eq(&config.forwards.name) => {
                        config.forwards.value = str_to_u64(config.forwards.name, value, 0, true)?
                    }
                    opt if opt.eq(&config.forwards_filter_amt_msat.name) => {
                        config.forwards_filter_amt_msat.value =
                            str_to_i64(config.forwards_filter_amt_msat.name, value, -1)?
                    }
                    opt if opt.eq(&config.forwards_filter_fee_msat.name) => {
                        config.forwards_filter_fee_msat.value =
                            str_to_i64(config.forwards_filter_fee_msat.name, value, -1)?
                    }
                    opt if opt.eq(&config.forwards_alias.name) => match value.parse::<bool>() {
                        Ok(b) => config.forwards_alias.value = b,
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse bool from `{}` for {}: {}",
                                value,
                                config.forwards_alias.name,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.pays.name) => {
                        config.pays.value = str_to_u64(config.pays.name, value, 0, true)?
                    }
                    opt if opt.eq(&config.invoices.name) => {
                        config.invoices.value = str_to_u64(config.invoices.name, value, 0, true)?
                    }
                    opt if opt.eq(&config.invoices_filter_amt_msat.name) => {
                        config.invoices_filter_amt_msat.value =
                            str_to_i64(config.invoices_filter_amt_msat.name, value, -1)?
                    }
                    opt if opt.eq(&config.locale.name) => match value.parse::<String>() {
                        Ok(s) => match Locale::from_str(&s) {
                            Ok(l) => config.locale.value = l,
                            Err(e) => {
                                return Err(anyhow!("Not a valid locale: {}", e));
                            }
                        },
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse locale as string: {}. {}",
                                value,
                                e
                            ));
                        }
                    },
                    opt if opt.eq(&config.refresh_alias.name) => {
                        config.refresh_alias.value =
                            str_to_u64(config.refresh_alias.name, value, 1, false)?
                    }
                    opt if opt.eq(&config.max_alias_length.name) => {
                        config.max_alias_length.value =
                            str_to_u64(config.max_alias_length.name, value, 5, false)?
                    }
                    opt if opt.eq(&config.availability_interval.name) => {
                        config.availability_interval.value =
                            str_to_u64(config.availability_interval.name, value, 1, false)?
                    }
                    opt if opt.eq(&config.availability_window.name) => {
                        config.availability_window.value =
                            str_to_u64(config.availability_window.name, value, 1, false)?
                    }
                    opt if opt.eq(&config.utf8.name) => match value.parse::<bool>() {
                        Ok(b) => config.utf8.value = b,
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse bool from `{}` for {}: {}",
                                value,
                                config.utf8.name,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.style.name) => {
                        config.style.value = Styles::from_str(value)?
                    }
                    opt if opt.eq(&config.flow_style.name) => {
                        config.flow_style.value = Styles::from_str(value)?
                    }
                    opt if opt.eq(&config.json.name) => match value.parse::<bool>() {
                        Ok(b) => config.json.value = b,
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse bool from `{}` for {}: {}",
                                value,
                                config.json.name,
                                e
                            ))
                        }
                    },
                    _ => (),
                }
            }
        }
    }
    Ok(())
}

pub fn get_startup_options(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
    state: PluginState,
) -> Result<(), Error> {
    {
        let mut config = state.config.lock();

        if let Some(cols) = plugin.option(&OPT_COLUMNS)? {
            config.columns.value = validate_columns_input(&cols)?
        };
        if let Some(sort_by) = plugin.option(&OPT_SORT_BY)? {
            config.sort_by.value = validate_sort_input(&sort_by)?
        };
        if let Some(cols) = plugin.option(&OPT_EXCLUDE_CHANNEL_STATES)? {
            let result = validate_exclude_states_input(&cols)?;
            config.exclude_channel_states.value = result.0;
            config.exclude_pub_priv_states = result.1;
        };
        if let Some(fws) = plugin.option(&OPT_FORWARDS)? {
            config.forwards.value = options_value_to_u64(&OPT_FORWARDS, fws, 0, true)?
        };
        if let Some(ffa) = plugin.option(&OPT_FORWARDS_FILTER_AMT)? {
            config.forwards_filter_amt_msat.value =
                validate_i64_input(ffa, OPT_FORWARDS_FILTER_AMT.name, -1)?
        };
        if let Some(fff) = plugin.option(&OPT_FORWARDS_FILTER_FEE)? {
            config.forwards_filter_fee_msat.value =
                validate_i64_input(fff, OPT_FORWARDS_FILTER_FEE.name, -1)?
        };
        if let Some(fa) = plugin.option(&OPT_FORWARDS_ALIAS)? {
            config.forwards_alias.value = fa
        };
        if let Some(pays) = plugin.option(&OPT_PAYS)? {
            config.pays.value = options_value_to_u64(&OPT_PAYS, pays, 0, true)?
        };
        if let Some(invs) = plugin.option(&OPT_INVOICES)? {
            config.invoices.value = options_value_to_u64(&OPT_INVOICES, invs, 0, true)?
        };
        if let Some(invfa) = plugin.option(&OPT_INVOICES_FILTER_AMT)? {
            config.invoices_filter_amt_msat.value =
                validate_i64_input(invfa, OPT_INVOICES_FILTER_AMT.name, -1)?
        };
        if let Some(loc) = plugin.option(&OPT_LOCALE)? {
            config.locale.value = match Locale::from_str(&loc) {
                Ok(l) => l,
                Err(e) => return Err(anyhow!("`{}` is not a valid locale: {}", loc, e)),
            }
        };
        if let Some(ra) = plugin.option(&OPT_REFRESH_ALIAS)? {
            config.refresh_alias.value = options_value_to_u64(&OPT_REFRESH_ALIAS, ra, 1, false)?
        };
        if let Some(mal) = plugin.option(&OPT_MAX_ALIAS_LENGTH)? {
            config.max_alias_length.value =
                options_value_to_u64(&OPT_MAX_ALIAS_LENGTH, mal, 5, false)?
        };
        if let Some(ai) = plugin.option(&OPT_AVAILABILITY_INTERVAL)? {
            config.availability_interval.value =
                options_value_to_u64(&OPT_AVAILABILITY_INTERVAL, ai, 1, false)?
        };
        if let Some(aw) = plugin.option(&OPT_AVAILABILITY_WINDOW)? {
            config.availability_window.value =
                options_value_to_u64(&OPT_AVAILABILITY_WINDOW, aw, 1, false)?
        };
        if let Some(utf8) = plugin.option(&OPT_UTF8)? {
            config.utf8.value = utf8
        };
        if let Some(style) = plugin.option(&OPT_STYLE)? {
            config.style.value = Styles::from_str(&style)?
        };
        if let Some(fstyle) = plugin.option(&OPT_FLOW_STYLE)? {
            config.flow_style.value = Styles::from_str(&fstyle)?
        };
        if let Some(js) = plugin.option(&OPT_JSON)? {
            config.json.value = js
        };
    }
    Ok(())
}

fn is_valid_hour_timestamp(val: u64) -> bool {
    Utc::now().timestamp() as u64 > val * 60 * 60
}
