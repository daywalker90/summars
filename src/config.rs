use anyhow::{anyhow, Error};
use chrono::Utc;
use cln_plugin::{options, ConfiguredPlugin};
use log::warn;
use std::path::Path;
use std::str::FromStr;
use tokio::fs;

use crate::{
    structs::{Config, Summary},
    PluginState,
};
use num_format::Locale;

pub fn validateargs(args: serde_json::Value, mut config: Config) -> Result<Config, Error> {
    if let serde_json::Value::Object(i) = args {
        for (key, value) in i.iter() {
            match key {
                name if name.eq(&config.show_pubkey.0) => match value {
                    serde_json::Value::Bool(b) => config.show_pubkey.1 = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.show_pubkey.0
                        ))
                    }
                },
                name if name.eq(&config.show_maxhtlc.0) => match value {
                    serde_json::Value::Bool(b) => config.show_maxhtlc.1 = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.show_maxhtlc.0
                        ))
                    }
                },
                name if name.eq(&config.sort_by.0) => match value {
                    serde_json::Value::String(b) => {
                        if Summary::FIELD_NAMES_AS_ARRAY.contains(&b.clone().as_str()) {
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
                name if name.eq(&config.forwards.0) => match value {
                    serde_json::Value::Number(b) => {
                        match b.as_u64() {
                            Some(n) => config.forwards.1 = n,
                            None => {
                                return Err(anyhow!(
                                    "Could not read a positive number for {}. Use 0 to disable.",
                                    config.forwards.0
                                ))
                            }
                        };
                    }
                    _ => {
                        return Err(anyhow!(
                            "{} must be a positive number. Use 0 to disable.",
                            config.forwards.0
                        ))
                    }
                },
                name if name.eq(&config.forward_alias.0) => match value {
                    serde_json::Value::Bool(b) => config.forward_alias.1 = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.forward_alias.0
                        ))
                    }
                },
                name if name.eq(&config.pays.0) => match value {
                    serde_json::Value::Number(b) => {
                        match b.as_u64() {
                            Some(n) => {
                                if is_valid_hour_timestamp(n) {
                                    config.pays.1 = n
                                } else {
                                    return Err(anyhow!(
                                        "Number is too big for {}.",
                                        config.pays.0
                                    ));
                                }
                            }
                            None => {
                                return Err(anyhow!(
                                    "Could not read a positive number for {}. Use 0 to disable.",
                                    config.pays.0
                                ))
                            }
                        };
                    }
                    _ => {
                        return Err(anyhow!(
                            "{} must be a positive number. Use 0 to disable.",
                            config.pays.0
                        ))
                    }
                },
                name if name.eq(&config.invoices.0) => match value {
                    serde_json::Value::Number(b) => {
                        match b.as_u64() {
                            Some(n) => {
                                if is_valid_hour_timestamp(n) {
                                    config.invoices.1 = n
                                } else {
                                    return Err(anyhow!(
                                        "Number is too big for {}.",
                                        config.invoices.0
                                    ));
                                }
                            }
                            None => {
                                return Err(anyhow!(
                                    "Could not read a positive number for {}. Use 0 to disable.",
                                    config.invoices.0
                                ))
                            }
                        };
                    }
                    _ => {
                        return Err(anyhow!(
                            "{} must be a positive number. Use 0 to disable.",
                            config.invoices.0
                        ))
                    }
                },
                name if name.eq(&config.locale.0) => match value {
                    serde_json::Value::String(s) => {
                        config.locale.1 = match Locale::from_str(s) {
                            Ok(l) => l,
                            Err(e) => return Err(anyhow!("Not a valid locale: {}. {}", s, e)),
                        }
                    }
                    _ => return Err(anyhow!("Not a valid string for: {}", config.locale.0)),
                },
                name if name.eq(&config.refresh_alias.0) => match value {
                    serde_json::Value::Number(b) => {
                        match b.as_u64() {
                            Some(n) => {
                                if n > 0 {
                                    config.refresh_alias.1 = n
                                } else {
                                    return Err(anyhow!(
                                        "Number needs to be greater than 0 for {}.",
                                        config.refresh_alias.0
                                    ));
                                }
                            }
                            None => {
                                return Err(anyhow!(
                                    "Could not read a positive number for {}.",
                                    config.refresh_alias.0
                                ))
                            }
                        };
                    }
                    _ => {
                        return Err(anyhow!(
                            "{} must be a positive number.",
                            config.refresh_alias.0
                        ))
                    }
                },
                name if name.eq(&config.max_alias_length.0) => match value {
                    serde_json::Value::Number(b) => {
                        match b.as_u64() {
                            Some(n) => {
                                if n > 0 {
                                    config.max_alias_length.1 = n as usize
                                } else {
                                    return Err(anyhow!(
                                        "Number needs to be greater than 0 for {}.",
                                        config.max_alias_length.0
                                    ));
                                }
                            }
                            None => {
                                return Err(anyhow!(
                                    "Could not read a positive number for {}.",
                                    config.max_alias_length.0
                                ))
                            }
                        };
                    }
                    _ => {
                        return Err(anyhow!(
                            "{} must be a positive number.",
                            config.max_alias_length.0
                        ))
                    }
                },
                name if name.eq(&config.availability_interval.0) => match value {
                    serde_json::Value::Number(b) => {
                        match b.as_u64() {
                            Some(n) => {
                                if n > 0 {
                                    config.availability_interval.1 = n
                                } else {
                                    return Err(anyhow!(
                                        "Number needs to be greater than 0 for {}.",
                                        config.availability_interval.0
                                    ));
                                }
                            }
                            None => {
                                return Err(anyhow!(
                                    "Could not read a positive number for {}.",
                                    config.availability_interval.0
                                ))
                            }
                        };
                    }
                    _ => {
                        return Err(anyhow!(
                            "{} must be a positive number.",
                            config.availability_interval.0
                        ))
                    }
                },
                name if name.eq(&config.availability_window.0) => match value {
                    serde_json::Value::Number(b) => {
                        match b.as_u64() {
                            Some(n) => {
                                if n > 0 {
                                    config.availability_window.1 = n
                                } else {
                                    return Err(anyhow!(
                                        "Number needs to be greater than 0 for {}.",
                                        config.availability_window.0
                                    ));
                                }
                            }
                            None => {
                                return Err(anyhow!(
                                    "Could not read a positive number for {}.",
                                    config.availability_window.0
                                ))
                            }
                        };
                    }
                    _ => {
                        return Err(anyhow!(
                            "{} must be a positive number.",
                            config.availability_window.0
                        ))
                    }
                },
                name if name.eq(&config.utf8.0) => match value {
                    serde_json::Value::Bool(b) => config.utf8.1 = *b,
                    _ => {
                        return Err(anyhow!(
                            "{} needs to be bool (true or false).",
                            config.utf8.0
                        ))
                    }
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
                    opt if opt.eq(&config.show_pubkey.0) => match value.parse::<bool>() {
                        Ok(b) => config.show_pubkey.1 = b,
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse bool from `{}` for {}: {}",
                                value,
                                config.show_pubkey.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.show_maxhtlc.0) => match value.parse::<bool>() {
                        Ok(b) => config.show_maxhtlc.1 = b,
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse bool from `{}` for {}: {}",
                                value,
                                config.show_maxhtlc.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.sort_by.0) => match value.parse::<String>() {
                        Ok(b) => {
                            if Summary::FIELD_NAMES_AS_ARRAY.contains(&b.clone().as_str()) {
                                config.sort_by.1 = b;
                            } else {
                                return Err(anyhow!(
                                    "Not a valid column name: `{}` for {}. Must be one of: {}",
                                    b,
                                    config.sort_by.0,
                                    Summary::field_names_to_string()
                                ));
                            }
                        }
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse column from `{}` for {}: {}",
                                value,
                                config.sort_by.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.forwards.0) => match value.parse::<u64>() {
                        Ok(n) => config.forwards.1 = n,
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse a positive number from `{}` for {}: {}",
                                value,
                                config.forwards.0,
                                e
                            ))
                        }
                    },
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
                    opt if opt.eq(&config.pays.0) => match value.parse::<u64>() {
                        Ok(n) => {
                            if is_valid_hour_timestamp(n) {
                                config.pays.1 = n
                            } else {
                                return Err(anyhow!(
                                    "`{}` is too big for {}",
                                    value,
                                    config.pays.0,
                                ));
                            }
                        }
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse a positive number from `{}` for {}: {}",
                                value,
                                config.pays.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.invoices.0) => match value.parse::<u64>() {
                        Ok(n) => {
                            if is_valid_hour_timestamp(n) {
                                config.invoices.1 = n
                            } else {
                                return Err(anyhow!(
                                    "`{}` is too big for {}",
                                    value,
                                    config.invoices.0,
                                ));
                            }
                        }
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse a positive number from `{}` for {}: {}",
                                value,
                                config.invoices.0,
                                e
                            ))
                        }
                    },
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
                    opt if opt.eq(&config.refresh_alias.0) => match value.parse::<u64>() {
                        Ok(n) => {
                            if n > 0 {
                                config.refresh_alias.1 = n
                            } else {
                                return Err(anyhow!(
                                    "Number needs to be greater than 0 for {}.",
                                    config.refresh_alias.0
                                ));
                            }
                        }
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse a positive number from `{}` for {}: {}",
                                value,
                                config.refresh_alias.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.max_alias_length.0) => match value.parse::<usize>() {
                        Ok(n) => {
                            if n > 0 {
                                config.max_alias_length.1 = n
                            } else {
                                return Err(anyhow!(
                                    "Number needs to be greater than 0 for {}.",
                                    config.max_alias_length.0
                                ));
                            }
                        }
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse a positive number from `{}` for {}: {}",
                                value,
                                config.max_alias_length.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.availability_interval.0) => match value.parse::<u64>() {
                        Ok(n) => {
                            if n > 0 {
                                config.availability_interval.1 = n
                            } else {
                                return Err(anyhow!(
                                    "Number needs to be greater than 0 for {}.",
                                    config.availability_interval.0
                                ));
                            }
                        }
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse a positive number from `{}` for {}: {}",
                                value,
                                config.availability_interval.0,
                                e
                            ))
                        }
                    },
                    opt if opt.eq(&config.availability_window.0) => match value.parse::<u64>() {
                        Ok(n) => {
                            if n > 0 {
                                config.availability_window.1 = n
                            } else {
                                return Err(anyhow!(
                                    "Number needs to be greater than 0 for {}.",
                                    config.availability_window.0
                                ));
                            }
                        }
                        Err(e) => {
                            return Err(anyhow!(
                                "Could not parse a positive number from `{}` for {}: {}",
                                value,
                                config.availability_window.0,
                                e
                            ))
                        }
                    },
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
                    _ => (),
                }
            }
        }
    }
    // for line in configfile {
    //     info!("{:?}", line);
    // }

    // log::info!("readconfig {:?}", config.show_pubkey.1);
    Ok(())
}

pub fn get_startup_options(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
    state: PluginState,
) -> Result<(), Error> {
    {
        let mut config = state.config.lock();
        config.show_pubkey.1 = match plugin.option(&config.show_pubkey.0) {
            Some(options::Value::Boolean(b)) => b,
            Some(_) => config.show_pubkey.1,
            None => config.show_pubkey.1,
        };
        config.show_maxhtlc.1 = match plugin.option(&config.show_maxhtlc.0) {
            Some(options::Value::Boolean(b)) => b,
            Some(_) => config.show_maxhtlc.1,
            None => config.show_maxhtlc.1,
        };
        config.sort_by.1 = match plugin.option(&config.sort_by.0) {
            Some(options::Value::String(s)) => {
                if Summary::FIELD_NAMES_AS_ARRAY.contains(&s.clone().as_str()) {
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
        config.forwards.1 = match plugin.option(&config.forwards.0) {
            Some(options::Value::Integer(i)) => {
                if i >= 0 {
                    i as u64
                } else {
                    return Err(anyhow!(
                        "{} needs to be a positive number and not `{}`. Use 0 to disable.",
                        config.forwards.0,
                        i
                    ));
                }
            }
            Some(_) => config.forwards.1,
            None => config.forwards.1,
        };
        config.forward_alias.1 = match plugin.option(&config.forward_alias.0) {
            Some(options::Value::Boolean(b)) => b,
            Some(_) => config.forward_alias.1,
            None => config.forward_alias.1,
        };
        config.pays.1 = match plugin.option(&config.pays.0) {
            Some(options::Value::Integer(i)) => {
                if is_valid_hour_timestamp(i as u64) {
                    i as u64
                } else {
                    return Err(anyhow!(
                        "{} needs to be a positive number and smaller than {}, \
                        not `{}`. Use 0 to disable.",
                        config.pays.0,
                        (Utc::now().timestamp() as u64) / 60 / 60,
                        i
                    ));
                }
            }
            Some(_) => config.pays.1,
            None => config.pays.1,
        };
        config.invoices.1 = match plugin.option(&config.invoices.0) {
            Some(options::Value::Integer(i)) => {
                if is_valid_hour_timestamp(i as u64) {
                    i as u64
                } else {
                    return Err(anyhow!(
                        "{} needs to be a positive number and smaller than {}, not `{}`. \
                        Use 0 to disable.",
                        config.invoices.0,
                        (Utc::now().timestamp() as u64) / 60 / 60,
                        i
                    ));
                }
            }
            Some(_) => config.invoices.1,
            None => config.invoices.1,
        };
        config.locale.1 = match plugin.option(&config.locale.0) {
            Some(options::Value::String(s)) => match Locale::from_str(&s) {
                Ok(l) => l,
                Err(e) => return Err(anyhow!("`{}` is not a valid locale: {}", s, e)),
            },
            Some(_) => config.locale.1,
            None => config.locale.1,
        };
        config.refresh_alias.1 = match plugin.option(&config.refresh_alias.0) {
            Some(options::Value::Integer(i)) => {
                if i > 0 {
                    i as u64
                } else {
                    return Err(anyhow!(
                        "{} needs to be greater than 0 and not `{}`.",
                        config.refresh_alias.0,
                        i
                    ));
                }
            }
            Some(_) => config.refresh_alias.1,
            None => config.refresh_alias.1,
        };
        config.max_alias_length.1 = match plugin.option(&config.max_alias_length.0) {
            Some(options::Value::Integer(i)) => {
                if i > 0 {
                    i as usize
                } else {
                    return Err(anyhow!(
                        "{} needs to be greater than 0 and not `{}`.",
                        config.max_alias_length.0,
                        i
                    ));
                }
            }
            Some(_) => config.max_alias_length.1,
            None => config.max_alias_length.1,
        };
        config.availability_interval.1 = match plugin.option(&config.availability_interval.0) {
            Some(options::Value::Integer(i)) => {
                if i > 0 {
                    i as u64
                } else {
                    return Err(anyhow!(
                        "{} needs to be greater than 0 and not `{}`.",
                        config.availability_interval.0,
                        i
                    ));
                }
            }
            Some(_) => config.availability_interval.1,
            None => config.availability_interval.1,
        };
        config.availability_window.1 = match plugin.option(&config.availability_window.0) {
            Some(options::Value::Integer(i)) => {
                if i > 0 {
                    i as u64
                } else {
                    return Err(anyhow!(
                        "{} needs to be greater than 0 and not `{}`.",
                        config.availability_window.0,
                        i
                    ));
                }
            }
            Some(_) => config.availability_window.1,
            None => config.availability_window.1,
        };
        config.utf8.1 = match plugin.option(&config.utf8.0) {
            Some(options::Value::Boolean(b)) => b,
            Some(_) => config.utf8.1,
            None => config.utf8.1,
        };
    }
    // log::info!("readconfig {:?}", config.show_pubkey.1);
    Ok(())
}

fn is_valid_hour_timestamp(pays: u64) -> bool {
    Utc::now().timestamp() as u64 > pays * 60 * 60
}
