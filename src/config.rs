use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::{options, ConfiguredPlugin};
use log::warn;
use std::path::Path;
use std::str::FromStr;
use tokio::fs;

use crate::{
    structs::{Config, Styles, Summary},
    PluginState,
};
use num_format::Locale;

fn validate_columns_input(input: &str) -> Result<Vec<String>, Error> {
    let cleaned_input: String = input.chars().filter(|&c| !c.is_whitespace()).collect();
    let split_input: Vec<&str> = cleaned_input.split(',').collect();

    for i in &split_input {
        if !Summary::get_field_names().contains(&i.to_string()) {
            return Err(anyhow!("`{}` not found in valid column names!", i));
        }
    }

    let cleaned_strings: Vec<String> = split_input.into_iter().map(String::from).collect();
    Ok(cleaned_strings)
}

fn validate_u64_input(
    n: u64,
    var_name: &String,
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

fn validate_i64_input(n: i64, var_name: &String, gteq: i64) -> Result<i64, Error> {
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
    config_var: &(String, u64),
    value: Option<options::Value>,
    gteq: u64,
    check_valid_time: bool,
) -> Result<u64, Error> {
    match value {
        Some(options::Value::Integer(i)) => {
            if i >= 0 {
                validate_u64_input(i as u64, &config_var.0, gteq, check_valid_time)
            } else {
                Err(anyhow!(
                    "{} needs to be a positive number and not `{}`.",
                    config_var.0,
                    i
                ))
            }
        }
        Some(_) => Ok(config_var.1),
        None => Ok(config_var.1),
    }
}

fn options_value_to_i64(
    config_var: &(String, i64),
    value: Option<options::Value>,
    gteq: i64,
) -> Result<i64, Error> {
    match value {
        Some(options::Value::Integer(i)) => validate_i64_input(i, &config_var.0, gteq),
        Some(_) => Ok(config_var.1),
        None => Ok(config_var.1),
    }
}

fn value_to_u64(
    var_name: &String,
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

fn value_to_i64(var_name: &String, value: &serde_json::Value, gteq: i64) -> Result<i64, Error> {
    match value {
        serde_json::Value::Number(b) => match b.as_i64() {
            Some(n) => validate_i64_input(n, var_name, gteq),
            None => Err(anyhow!("Could not read a number for {}.", var_name)),
        },
        _ => Err(anyhow!("{} must be a number.", var_name)),
    }
}

fn str_to_u64(
    var_name: &String,
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

fn str_to_i64(var_name: &String, value: &str, gteq: i64) -> Result<i64, Error> {
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

pub fn validateargs(args: serde_json::Value, mut config: Config) -> Result<Config, Error> {
    if let serde_json::Value::Object(i) = args {
        for (key, value) in i.iter() {
            match key {
                name if name.eq(&config.columns.0) => match value {
                    serde_json::Value::String(b) => {
                        config.columns.1 = validate_columns_input(b)?;
                    }
                    _ => {
                        return Err(anyhow!(
                            "Not a string. {} must be a comma separated string of: {}",
                            config.columns.0,
                            Summary::field_names_to_string()
                        ))
                    }
                },
                name if name.eq(&config.sort_by.0) => match value {
                    serde_json::Value::String(b) => {
                        if Summary::get_field_names().contains(b) {
                            config.sort_by.1 = b.to_string()
                        } else {
                            return Err(anyhow!(
                                "Not a valid column name: `{}`. Must be one of: {}",
                                b,
                                Summary::field_names_to_string()
                            ));
                        }
                    }
                    _ => {
                        return Err(anyhow!(
                            "Not a string. {} must be one of: {}",
                            config.sort_by.0,
                            Summary::field_names_to_string()
                        ))
                    }
                },
                name if name.eq(&config.forwards.0) => {
                    config.forwards.1 = value_to_u64(&config.forwards.0, value, 0, true)?
                }
                name if name.eq(&config.forwards_filter_amt_msat.0) => {
                    config.forwards_filter_amt_msat.1 =
                        value_to_i64(&config.forwards_filter_amt_msat.0, value, -1)?
                }
                name if name.eq(&config.forwards_filter_fee_msat.0) => {
                    config.forwards_filter_fee_msat.1 =
                        value_to_i64(&config.forwards_filter_fee_msat.0, value, -1)?
                }
                name if name.eq(&config.forward_alias.0) => match value {
                    serde_json::Value::Bool(b) => config.forward_alias.1 = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.forward_alias.0
                        ))
                    }
                },
                name if name.eq(&config.pays.0) => {
                    config.pays.1 = value_to_u64(&config.pays.0, value, 0, true)?
                }
                name if name.eq(&config.invoices.0) => {
                    config.invoices.1 = value_to_u64(&config.invoices.0, value, 0, true)?
                }
                name if name.eq(&config.invoices_filter_amt_msat.0) => {
                    config.invoices_filter_amt_msat.1 =
                        value_to_i64(&config.invoices_filter_amt_msat.0, value, -1)?
                }
                name if name.eq(&config.locale.0) => match value {
                    serde_json::Value::String(s) => {
                        config.locale.1 = match Locale::from_str(s) {
                            Ok(l) => l,
                            Err(e) => return Err(anyhow!("Not a valid locale: {}. {}", s, e)),
                        }
                    }
                    _ => return Err(anyhow!("Not a valid string for: {}", config.locale.0)),
                },
                name if name.eq(&config.refresh_alias.0) => {
                    config.refresh_alias.1 = value_to_u64(&config.refresh_alias.0, value, 1, false)?
                }
                name if name.eq(&config.max_alias_length.0) => {
                    config.max_alias_length.1 =
                        value_to_u64(&config.max_alias_length.0, value, 5, false)?
                }
                name if name.eq(&config.availability_interval.0) => {
                    config.availability_interval.1 =
                        value_to_u64(&config.availability_interval.0, value, 1, false)?
                }
                name if name.eq(&config.availability_window.0) => {
                    config.availability_window.1 =
                        value_to_u64(&config.availability_window.0, value, 1, false)?
                }
                name if name.eq(&config.utf8.0) => match value {
                    serde_json::Value::Bool(b) => config.utf8.1 = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.utf8.0
                        ))
                    }
                },
                name if name.eq(&config.style.0) => match value {
                    serde_json::Value::String(s) => {
                        config.style.1 = Styles::from_str(s)?;
                    }
                    _ => return Err(anyhow!("Not a valid string for: {}", config.style.0)),
                },
                name if name.eq(&config.flow_style.0) => match value {
                    serde_json::Value::String(s) => {
                        config.flow_style.1 = Styles::from_str(s)?;
                    }
                    _ => return Err(anyhow!("Not a valid string for: {}", config.flow_style.0)),
                },
                other => return Err(anyhow!("option not found:{:?}", other)),
            };
        }
    };
    Ok(config)
}

pub async fn read_config(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
    state: PluginState,
) -> Result<(), Error> {
    let mut configfile = String::new();
    let dir = plugin.configuration().clone().lightning_dir;
    match fs::read_to_string(Path::new(&dir).join("config")).await {
        Ok(file) => configfile = file,
        Err(_) => {
            match fs::read_to_string(Path::new(&dir).parent().unwrap().join("config")).await {
                Ok(file2) => configfile = file2,
                Err(_) => warn!("No config file found!"),
            }
        }
    }
    let mut config = state.config.lock();
    for line in configfile.lines() {
        if line.contains('=') {
            let splitline = line.split('=').collect::<Vec<&str>>();
            if splitline.len() == 2 {
                let name = splitline.first().unwrap();
                let value = splitline.get(1).unwrap();

                match name {
                    opt if opt.eq(&config.columns.0) => {
                        config.columns.1 = validate_columns_input(value)?
                    }
                    opt if opt.eq(&config.sort_by.0) => {
                        if Summary::get_field_names().contains(&value.to_string()) {
                            config.sort_by.1 = value.to_string();
                        } else {
                            return Err(anyhow!(
                                "Not a valid column name: `{}` for {}. Must be one of: {}",
                                value,
                                config.sort_by.0,
                                Summary::field_names_to_string()
                            ));
                        }
                    }
                    opt if opt.eq(&config.forwards.0) => {
                        config.forwards.1 = str_to_u64(&config.forwards.0, value, 0, true)?
                    }
                    opt if opt.eq(&config.forwards_filter_amt_msat.0) => {
                        config.forwards_filter_amt_msat.1 =
                            str_to_i64(&config.forwards_filter_amt_msat.0, value, -1)?
                    }
                    opt if opt.eq(&config.forwards_filter_fee_msat.0) => {
                        config.forwards_filter_fee_msat.1 =
                            str_to_i64(&config.forwards_filter_fee_msat.0, value, -1)?
                    }
                    opt if opt.eq(&config.forward_alias.0) => match value.parse::<bool>() {
                        Ok(b) => config.forward_alias.1 = b,
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse bool from `{}` for {}: {}",
                                value,
                                config.forward_alias.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.pays.0) => {
                        config.pays.1 = str_to_u64(&config.pays.0, value, 0, true)?
                    }
                    opt if opt.eq(&config.invoices.0) => {
                        config.invoices.1 = str_to_u64(&config.invoices.0, value, 0, true)?
                    }
                    opt if opt.eq(&config.invoices_filter_amt_msat.0) => {
                        config.invoices_filter_amt_msat.1 =
                            str_to_i64(&config.invoices_filter_amt_msat.0, value, -1)?
                    }
                    opt if opt.eq(&config.locale.0) => match value.parse::<String>() {
                        Ok(s) => match Locale::from_name(s) {
                            Ok(l) => config.locale.1 = l,
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
                    opt if opt.eq(&config.refresh_alias.0) => {
                        config.refresh_alias.1 =
                            str_to_u64(&config.refresh_alias.0, value, 1, false)?
                    }
                    opt if opt.eq(&config.max_alias_length.0) => {
                        config.max_alias_length.1 =
                            str_to_u64(&config.max_alias_length.0, value, 5, false)?
                    }
                    opt if opt.eq(&config.availability_interval.0) => {
                        config.availability_interval.1 =
                            str_to_u64(&config.availability_interval.0, value, 1, false)?
                    }
                    opt if opt.eq(&config.availability_window.0) => {
                        config.availability_window.1 =
                            str_to_u64(&config.availability_window.0, value, 1, false)?
                    }
                    opt if opt.eq(&config.utf8.0) => match value.parse::<bool>() {
                        Ok(b) => config.utf8.1 = b,
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse bool from `{}` for {}: {}",
                                value,
                                config.utf8.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.style.0) => config.style.1 = Styles::from_str(value)?,
                    opt if opt.eq(&config.flow_style.0) => {
                        config.flow_style.1 = Styles::from_str(value)?
                    }
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
        if let Some(options::Value::String(b)) = plugin.option(&config.columns.0) {
            config.columns.1 = validate_columns_input(&b)?;
        }
        config.sort_by.1 = match plugin.option(&config.sort_by.0) {
            Some(options::Value::String(s)) => {
                if Summary::get_field_names().contains(&s) {
                    s
                } else {
                    return Err(anyhow!(
                        "Not a valid column name: `{}`. Must be one of {}",
                        s,
                        Summary::field_names_to_string()
                    ));
                }
            }
            Some(_) => config.sort_by.1.clone(),
            None => config.sort_by.1.clone(),
        };
        config.forwards.1 =
            options_value_to_u64(&config.forwards, plugin.option(&config.forwards.0), 0, true)?;
        config.forwards_filter_amt_msat.1 = options_value_to_i64(
            &config.forwards_filter_amt_msat,
            plugin.option(&config.forwards_filter_amt_msat.0),
            -1,
        )?;
        config.forwards_filter_fee_msat.1 = options_value_to_i64(
            &config.forwards_filter_fee_msat,
            plugin.option(&config.forwards_filter_fee_msat.0),
            -1,
        )?;
        config.forward_alias.1 = match plugin.option(&config.forward_alias.0) {
            Some(options::Value::Boolean(b)) => b,
            Some(_) => config.forward_alias.1,
            None => config.forward_alias.1,
        };
        config.pays.1 = options_value_to_u64(&config.pays, plugin.option(&config.pays.0), 0, true)?;
        config.invoices.1 =
            options_value_to_u64(&config.invoices, plugin.option(&config.invoices.0), 0, true)?;
        config.invoices_filter_amt_msat.1 = options_value_to_i64(
            &config.invoices_filter_amt_msat,
            plugin.option(&config.invoices_filter_amt_msat.0),
            -1,
        )?;
        config.locale.1 = match plugin.option(&config.locale.0) {
            Some(options::Value::String(s)) => match Locale::from_str(&s) {
                Ok(l) => l,
                Err(e) => return Err(anyhow!("`{}` is not a valid locale: {}", s, e)),
            },
            Some(_) => config.locale.1,
            None => config.locale.1,
        };
        config.refresh_alias.1 = options_value_to_u64(
            &config.refresh_alias,
            plugin.option(&config.refresh_alias.0),
            1,
            false,
        )?;
        config.max_alias_length.1 = options_value_to_u64(
            &config.max_alias_length,
            plugin.option(&config.max_alias_length.0),
            5,
            false,
        )?;
        config.availability_interval.1 = options_value_to_u64(
            &config.availability_interval,
            plugin.option(&config.availability_interval.0),
            1,
            false,
        )?;
        config.availability_window.1 = options_value_to_u64(
            &config.availability_window,
            plugin.option(&config.availability_window.0),
            1,
            false,
        )?;
        config.utf8.1 = match plugin.option(&config.utf8.0) {
            Some(options::Value::Boolean(b)) => b,
            Some(_) => config.utf8.1,
            None => config.utf8.1,
        };
        config.style.1 = match plugin.option(&config.style.0) {
            Some(options::Value::String(s)) => Styles::from_str(&s)?,
            Some(_) => config.style.1.clone(),
            None => config.style.1.clone(),
        };
        config.flow_style.1 = match plugin.option(&config.flow_style.0) {
            Some(options::Value::String(s)) => Styles::from_str(&s)?,
            Some(_) => config.flow_style.1.clone(),
            None => config.flow_style.1.clone(),
        };
    }
    Ok(())
}

fn is_valid_hour_timestamp(val: u64) -> bool {
    Utc::now().timestamp() as u64 > val * 60 * 60
}
