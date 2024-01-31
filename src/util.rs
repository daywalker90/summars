use std::path::{Path, PathBuf};

use cln_plugin::Plugin;
use cln_rpc::model::responses::{ListpeerchannelsChannels, ListpeerchannelsChannelsState};

use crate::structs::{Config, PluginState};

pub fn is_active_state(channel: &ListpeerchannelsChannels) -> bool {
    #[allow(clippy::match_like_matches_macro)]
    match channel.state.unwrap() {
        ListpeerchannelsChannelsState::OPENINGD => true,
        ListpeerchannelsChannelsState::CHANNELD_AWAITING_LOCKIN => true,
        ListpeerchannelsChannelsState::CHANNELD_NORMAL => true,
        ListpeerchannelsChannelsState::DUALOPEND_OPEN_INIT => true,
        ListpeerchannelsChannelsState::DUALOPEND_AWAITING_LOCKIN => true,
        ListpeerchannelsChannelsState::CHANNELD_AWAITING_SPLICE => true,
        _ => false,
    }
}

pub fn make_channel_flags(private: Option<bool>, connected: bool) -> String {
    let mut flags = String::from("[");
    match private {
        Some(is_priv) => {
            if is_priv {
                flags.push('P')
            } else {
                flags.push('_')
            }
        }
        None => flags.push('E'),
    }
    if connected {
        flags.push('_')
    } else {
        flags.push('O')
    }
    flags.push(']');
    flags
}

#[derive(Debug)]
struct Charset {
    double_left: String,
    left: String,
    bar: String,
    mid: String,
    right: String,
    double_right: String,
    empty: String,
}

pub fn draw_chans_graph(
    config: &Config,
    total_msat: u64,
    to_us_msat: u64,
    graph_max_chan_side_msat: u64,
) -> String {
    let draw_utf8 = Charset {
        double_left: "╟".to_string(),
        left: "├".to_string(),
        bar: "─".to_string(),
        mid: "┼".to_string(),
        right: "┤".to_string(),
        double_right: "╢".to_string(),
        empty: "║".to_string(),
    };

    let draw_ascii = Charset {
        double_left: "#".to_string(),
        left: "[".to_string(),
        bar: "-".to_string(),
        mid: "+".to_string(),
        right: "]".to_string(),
        double_right: "#".to_string(),
        empty: "|".to_string(),
    };

    let our_len = ((to_us_msat as f64 / graph_max_chan_side_msat as f64) * 23.0).round() as usize;
    let their_len = (((total_msat - to_us_msat) as f64 / graph_max_chan_side_msat as f64) * 23.0)
        .round() as usize;

    let draw = if config.utf8.1 {
        &draw_utf8
    } else {
        &draw_ascii
    };

    let mut mid = draw.mid.clone();
    let left;
    let right;

    if our_len == 0 {
        left = format!("{:>23}", "");
        mid = draw.double_left.clone();
    } else {
        left = format!("{:>23}", draw.left.clone() + &draw.bar.repeat(our_len - 1));
    }

    if their_len == 0 {
        right = format!("{:23}", "");
        // Both 0 is a special case.
        if our_len == 0 {
            mid = draw.empty.clone();
        } else {
            mid = draw.double_right.clone();
        }
    } else {
        right = format!("{:23}", draw.bar.repeat(their_len - 1) + &draw.right);
    }

    format!("{}{}{}", left, mid, right)
}

#[test]
fn test_flags() {
    assert_eq!(make_channel_flags(Some(false), true), "[__]");
    assert_eq!(make_channel_flags(Some(true), true), "[P_]");
    assert_eq!(make_channel_flags(Some(false), false), "[_O]");
    assert_eq!(make_channel_flags(Some(true), false), "[PO]");
    assert_eq!(make_channel_flags(None, true), "[E_]");
    assert_eq!(make_channel_flags(None, false), "[EO]");
}

pub fn make_rpc_path(plugin: &Plugin<PluginState>) -> PathBuf {
    Path::new(&plugin.configuration().lightning_dir).join(plugin.configuration().rpc_file)
}
