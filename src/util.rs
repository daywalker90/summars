use std::path::{Path, PathBuf};

use cln_plugin::Plugin;
use cln_rpc::model::responses::{ListpeerchannelsChannels, ListpeerchannelsChannelsState};

use crate::structs::PluginState;

pub fn is_active_state(channel: &ListpeerchannelsChannels) -> bool {
    match channel.state.unwrap() {
        ListpeerchannelsChannelsState::OPENINGD => true,
        ListpeerchannelsChannelsState::CHANNELD_AWAITING_LOCKIN => true,
        ListpeerchannelsChannelsState::CHANNELD_NORMAL => true,
        ListpeerchannelsChannelsState::DUALOPEND_OPEN_INIT => true,
        ListpeerchannelsChannelsState::DUALOPEND_AWAITING_LOCKIN => true,
        _ => false,
    }
}

pub fn make_channel_flags(private: Option<bool>, connected: bool) -> String {
    let mut flags = String::from("[");
    match private {
        Some(is_priv) => {
            if is_priv {
                flags.push_str("P")
            } else {
                flags.push_str("_")
            }
        }
        None => flags.push_str("E"),
    }
    if connected {
        flags.push_str("_")
    } else {
        flags.push_str("O")
    }
    flags.push_str("]");
    flags
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
