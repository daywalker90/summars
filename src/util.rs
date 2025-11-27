use std::{
    collections::BTreeMap,
    fmt::Write,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::anyhow;
use chrono::{Datelike, Local, Timelike};
use cln_plugin::{Error, Plugin};
use cln_rpc::{
    model::{requests::ListnodesRequest, responses::ListpeerchannelsChannels},
    primitives::{ChannelState, PublicKey, ShortChannelId},
    ClnRpc,
};
use fixed_decimal::{FixedInteger, Sign, UnsignedDecimal};
use icu_datetime::{fieldsets, DateTimeFormatter};
use icu_decimal::{options::DecimalFormatterOptions, DecimalFormatter};
use tabled::grid::records::{
    vec_records::{Text, VecRecords},
    Resizable,
};

use crate::structs::{
    Config,
    GraphCharset,
    PluginState,
    TableColumn,
    NODE_GOSSIP_MISS,
    NO_ALIAS_SET,
};

pub fn is_active_state(channel: &ListpeerchannelsChannels) -> bool {
    #[allow(clippy::match_like_matches_macro)]
    match channel.state {
        ChannelState::OPENINGD => true,
        ChannelState::CHANNELD_AWAITING_LOCKIN => true,
        ChannelState::CHANNELD_NORMAL => true,
        ChannelState::DUALOPEND_OPEN_INIT => true,
        ChannelState::DUALOPEND_AWAITING_LOCKIN => true,
        ChannelState::CHANNELD_AWAITING_SPLICE => true,
        _ => false,
    }
}

pub fn make_channel_flags(private: bool, offline: bool) -> String {
    let mut flags = String::from("[");
    if private {
        flags.push('P');
    } else {
        flags.push('_');
    }

    if offline {
        flags.push('O');
    } else {
        flags.push('_');
    }
    flags.push(']');
    flags
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
#[allow(clippy::cast_precision_loss)]
pub fn draw_chans_graph(
    config: &Config,
    total_msat: u64,
    to_us_msat: u64,
    graph_max_chan_side_msat: u64,
) -> String {
    let draw_utf8 = GraphCharset::new_utf8();
    let draw_ascii = GraphCharset::new_ascii();

    let our_len = ((to_us_msat as f64 / graph_max_chan_side_msat as f64) * 23.0).round() as usize;
    let their_len = (((total_msat - to_us_msat) as f64 / graph_max_chan_side_msat as f64) * 23.0)
        .round() as usize;

    let draw = if config.utf8 { &draw_utf8 } else { &draw_ascii };

    let mut mid = draw.mid.clone();
    let left;
    let right;

    if our_len == 0 {
        left = format!("{:>23}", "");
        mid.clone_from(&draw.double_left);
    } else {
        left = format!("{:>23}", draw.left.clone() + &draw.bar.repeat(our_len - 1));
    }

    if their_len == 0 {
        right = format!("{:23}", "");
        // Both 0 is a special case.
        if our_len == 0 {
            mid.clone_from(&draw.empty);
        } else {
            mid.clone_from(&draw.double_right);
        }
    } else {
        right = format!("{:23}", draw.bar.repeat(their_len - 1) + &draw.right);
    }

    format!("{left}{mid}{right}")
}

pub fn u64_to_btc_string(config: &Config, amount_msat: u64) -> Result<String, Error> {
    let fixed_decimal_formatter = match DecimalFormatter::try_new(
        config.locale.clone().into(),
        DecimalFormatterOptions::default(),
    ) {
        Ok(fmt) => fmt,
        Err(e) => {
            return Err(anyhow!(
                "Could not create DecimalFormatter: locale invalid? {e}"
            ))
        }
    };
    let mut fixed_decimal = UnsignedDecimal::from(amount_msat);
    fixed_decimal = fixed_decimal.multiplied_pow10(-11);
    fixed_decimal = fixed_decimal.trunced(-8);
    fixed_decimal = fixed_decimal.padded_end(-8);
    Ok(format!(
        "{}",
        fixed_decimal_formatter.format(&fixed_decimal::Signed::new(Sign::None, fixed_decimal))
    ))
}

pub fn u64_to_sat_string(config: &Config, amount_sat: u64) -> Result<String, Error> {
    let fixed_decimal_formatter = match DecimalFormatter::try_new(
        config.locale.clone().into(),
        DecimalFormatterOptions::default(),
    ) {
        Ok(fmt) => fmt,
        Err(e) => {
            return Err(anyhow!(
                "Could not create DecimalFormatter: locale invalid? {e}"
            ))
        }
    };
    let fixed_decimal = FixedInteger::from(amount_sat);
    Ok(format!(
        "{}",
        fixed_decimal_formatter.format(&fixed_decimal.into())
    ))
}

pub fn timestamp_to_localized_datetime_string(
    config: &Config,
    timestamp: u64,
) -> Result<String, Error> {
    let d = UNIX_EPOCH + Duration::from_secs(timestamp);
    let datetime = chrono::DateTime::<Local>::from(d);
    let date_time_formatter =
        match DateTimeFormatter::try_new(config.locale.clone().into(), fieldsets::YMDT::short()) {
            Ok(d) => d,
            Err(e) => return Err(anyhow!("Could not create DateTimeFormatter: {e}")),
        };
    let datetime_iso = icu_time::DateTime {
        date: icu_calendar::Date::try_new_gregorian(
            datetime.year(),
            u8::try_from(datetime.month())?,
            u8::try_from(datetime.day())?,
        )?,
        time: icu_time::Time::try_new(
            u8::try_from(datetime.hour())?,
            u8::try_from(datetime.minute())?,
            u8::try_from(datetime.second())?,
            datetime.nanosecond(),
        )?,
    };
    match date_time_formatter.format_same_calendar(&datetime_iso) {
        Ok(fstr) => Ok(fstr.to_string()),
        Err(e) => Err(anyhow!("Could not format datetime string :{e}")),
    }
}

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut hex_string = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut hex_string, "{byte:02x}").unwrap();
    }
    hex_string
}

pub fn sort_columns<C: TableColumn>(
    records: &mut VecRecords<Text<String>>,
    headers: &[C],
    config_columns: &[C],
) {
    let mut target_index_map = Vec::new();

    for head in headers {
        for (j, prehead) in config_columns.iter().enumerate() {
            if head == prehead {
                target_index_map.push(j);
                break;
            }
        }
    }

    for i in 0..target_index_map.len() {
        while target_index_map[i] != i {
            let target_index = target_index_map[i];
            target_index_map.swap(i, target_index);
            records.swap_column(i, target_index);
        }
    }
}

pub fn at_or_above_version(my_version: &str, min_version: &str) -> Result<bool, Error> {
    let clean_start_my_version = my_version
        .split_once('v')
        .ok_or_else(|| anyhow!("Could not find v in version string"))?
        .1;
    let full_clean_my_version: String = clean_start_my_version
        .chars()
        .take_while(|x| x.is_ascii_digit() || *x == '.')
        .collect();

    let my_version_parts: Vec<&str> = full_clean_my_version.split('.').collect();
    let min_version_parts: Vec<&str> = min_version.split('.').collect();

    if my_version_parts.len() <= 1 || my_version_parts.len() > 3 {
        return Err(anyhow!("Version string parse error: {my_version}"));
    }
    for (my, min) in my_version_parts.iter().zip(min_version_parts.iter()) {
        let my_num: u32 = my.parse()?;
        let min_num: u32 = min.parse()?;

        if my_num != min_num {
            return Ok(my_num > min_num);
        }
    }

    Ok(my_version_parts.len() >= min_version_parts.len())
}

pub async fn get_alias(
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
    peer_id: PublicKey,
) -> Result<String, Error> {
    let alias = plugin
        .state()
        .alias_map
        .lock()
        .get::<PublicKey>(&peer_id)
        .cloned();
    match alias {
        Some(a) => Ok(a),
        None => {
            if let Some(node) = rpc
                .call_typed(&ListnodesRequest { id: Some(peer_id) })
                .await?
                .nodes
                .first()
            {
                let new_alias = match &node.alias {
                    Some(newalias) => newalias,
                    None => NO_ALIAS_SET,
                };
                plugin
                    .state()
                    .alias_map
                    .lock()
                    .insert(peer_id, new_alias.to_owned());
                Ok(new_alias.to_owned())
            } else {
                let new_alias = NODE_GOSSIP_MISS;
                plugin
                    .state()
                    .alias_map
                    .lock()
                    .insert(peer_id, new_alias.to_owned());
                Ok(new_alias.to_owned())
            }
        }
    }
}

pub async fn get_alias_from_scid(
    scid: ShortChannelId,
    chanmap: &BTreeMap<ShortChannelId, ListpeerchannelsChannels>,
    rpc: &mut ClnRpc,
    plugin: Plugin<PluginState>,
) -> String {
    let Some(listpeerchannel) = chanmap.get(&scid) else {
        return scid.to_string();
    };
    let Ok(alias) = get_alias(rpc, plugin, listpeerchannel.peer_id).await else {
        return scid.to_string();
    };
    if alias == NO_ALIAS_SET || alias == NODE_GOSSIP_MISS {
        scid.to_string()
    } else {
        alias
    }
}

#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
pub fn feeppm_effective_from_amts(amount_msat_start: u64, amount_msat_end: u64) -> u32 {
    assert!(
        amount_msat_start >= amount_msat_end,
        "CRITICAL ERROR: amount_msat_start should be greater than or equal to amount_msat_end"
    );
    ((amount_msat_start - amount_msat_end) as f64 / amount_msat_end as f64 * 1_000_000.0).ceil()
        as u32
}

pub fn replace_escaping_chars(s: &str) -> String {
    let mut result = String::new();

    for c in s.chars() {
        let replacement = match c {
            '"' => '\'',
            '\\' => '/',
            '\n' | '\t' | '\r' => ' ',
            _ if c.is_control() => '_',
            _ => c,
        };
        result.push(replacement);
    }

    result
}

#[macro_export]
macro_rules! impl_table_column {
    (
        $enum_name:ty,
        env_var = $env_var:expr,
        exclude_default = [$($exclude:ident),+ $(,)?],
        numerical = [$($numerical:ident),+ $(,)?],
        optional_numerical = [$($optional_numerical:ident),* $(,)?],
    ) => {
        impl $crate::structs::TableColumn for $enum_name {
            const NUMERICAL: &'static [Self] = &[$(Self::$numerical),+];
            const OPTIONAL_NUMERICAL: &'static [Self] = &[$(Self::$optional_numerical),*];

            fn default_columns() -> Vec<Self> {
                let mut all = <$enum_name as strum::IntoEnumIterator>::iter().collect::<Vec<_>>();
                all.retain(|c| {
                    !($(c == &Self::$exclude)||*)
                });
                all
            }

            fn parse_column(input: &str) -> Result<Self, Error> {
                Self::from_str(input).map_err(|_| {
                    anyhow!("`{}` not found in valid {} names!", input, $env_var)
                })
            }

            fn parse_columns(input: &str) -> Result<Vec<Self>, Error> {
                let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
                let parts: Vec<&str> = cleaned.split(',').filter(|s| !s.is_empty()).collect();

                let mut seen = std::collections::HashSet::new();
                for &part in &parts {
                    if !seen.insert(part) {
                        return Err(anyhow::anyhow!(
                            "Duplicate entry detected in {}: {}",
                            $env_var, part
                        ));
                    }
                }

                parts.iter()
                    .map(|&s| Self::parse_column(s))
                    .collect()
            }

            // fn all_list_string() -> String {
            //     use strum::IntoEnumIterator;
            //     <$enum_name as IntoEnumIterator>::iter()
            //         .map(|c| c.to_string())
            //         .collect::<Vec<String>>()
            //         .join(",")
            // }

            fn to_list_string(columns: &[Self]) -> String {
                columns.iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<String>>()
                    .join(",")
            }
        }
    };
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
#[inline]
pub const fn f64_to_u64_trunc(f: f64) -> u64 {
    f as u64
}

#[allow(clippy::cast_precision_loss)]
#[inline]
pub const fn perc_trunc_u64(dividend: u64, divisor: u64) -> f64 {
    (dividend as f64 / divisor as f64) * 100.0
}

#[inline]
pub const fn rounded_div_u64(dividend: u64, divisor: u64) -> u64 {
    let quotient = dividend / divisor;
    let remainder = dividend % divisor;
    quotient + (remainder << 1 >= divisor) as u64
}

#[test]
fn test_flags() {
    assert_eq!(make_channel_flags(false, false), "[__]");
    assert_eq!(make_channel_flags(true, false), "[P_]");
    assert_eq!(make_channel_flags(false, true), "[_O]");
    assert_eq!(make_channel_flags(true, true), "[PO]");
}

pub fn make_rpc_path(plugin: &Plugin<PluginState>) -> PathBuf {
    Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file)
}
