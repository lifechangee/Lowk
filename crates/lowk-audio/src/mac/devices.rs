//! Audio device queries: microphones, default output (for the tap's sub-device, rate change
//! and headphone detection).
use cidre::cf;
use cidre::core_audio::{self as ca, PropAddr, PropScope, PropSelector};
use lowk_types::{AppError, DeviceInfo, ErrorCode, Result};

fn err(what: &str, e: impl std::fmt::Debug) -> AppError {
    AppError::new(ErrorCode::AudioDevice).with_detail(format!("{what}: {e:?}"))
}

fn has_input(d: &ca::Device) -> bool {
    d.prop_vec::<ca::Obj>(&PropSelector::DEVICE_STREAMS.input_addr())
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Number of output streams a device exposes (0 for a tap-only aggregate: it cannot play).
pub fn output_stream_count(d: &ca::Device) -> usize {
    d.prop_vec::<ca::Obj>(&PropSelector::DEVICE_STREAMS.output_addr())
        .map(|v| v.len())
        .unwrap_or(0)
}

pub fn list_mics() -> Result<Vec<DeviceInfo>> {
    let def = ca::System::default_input_device()
        .ok()
        .and_then(|d| d.uid().ok())
        .map(|s| s.to_string());
    let mut out = vec![];
    for d in ca::System::devices().map_err(|e| err("device list", e))? {
        if !has_input(&d) {
            continue;
        }
        let (Ok(uid), Ok(name)) = (d.uid(), d.name()) else {
            continue;
        };
        let uid = uid.to_string();
        if uid.starts_with("CADefaultDeviceAggregate") || uid.starts_with("com.lowk.") {
            continue;
        }
        out.push(DeviceInfo {
            is_default: def.as_deref() == Some(uid.as_str()),
            id: uid,
            name: name.to_string(),
        });
    }
    out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Names of the Lowk tap aggregates alive in this process (must be empty after capture stops).
pub fn lowk_aggregates() -> Vec<String> {
    ca::System::devices()
        .map(|v| {
            v.iter()
                .filter_map(|d| d.name().ok().map(|n| n.to_string()))
                .filter(|n| n == "LowkTap")
                .collect()
        })
        .unwrap_or_default()
}

/// UID of the current default input device.
pub fn default_input_uid() -> Option<String> {
    ca::System::default_input_device()
        .ok()?
        .uid()
        .ok()
        .map(|s| s.to_string())
}

pub fn device_by_uid(uid: &str) -> Option<ca::Device> {
    let s = cf::String::from_str(uid);
    ca::Device::with_uid(&s).ok().filter(|d| !d.is_unknown())
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutInfo {
    pub uid: String,
    pub rate: f64,
    /// Headphones / headset (no acoustic path from the output to the mic).
    pub headphones: bool,
}

const SSRC: PropSelector = PropSelector(u32::from_be_bytes(*b"ssrc"));
const DATA_SRC_HEADPHONES: u32 = u32::from_be_bytes(*b"hdpn");

pub fn default_output() -> Option<OutInfo> {
    let d = ca::System::default_output_device().ok()?;
    let uid = d.uid().ok()?.to_string();
    let rate = d.actual_sample_rate().unwrap_or(0.0);
    let transport = d.transport_type().ok();
    let headphones = match transport {
        Some(t) if t == ca::DeviceTransportType::BLUETOOTH || t == ca::DeviceTransportType::BLUETOOTH_LE => {
            true
        }
        Some(t) if t == ca::DeviceTransportType::BUILT_IN => {
            let addr = PropAddr {
                selector: SSRC,
                scope: PropScope::OUTPUT,
                element: Default::default(),
            };
            d.prop::<u32>(&addr)
                .map(|v| v == DATA_SRC_HEADPHONES)
                .unwrap_or(false)
        }
        _ => false,
    };
    Some(OutInfo {
        uid,
        rate,
        headphones,
    })
}

/// Why [`auto_mic_uid`] chose what it chose (for logs and tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicChoice {
    /// Keep the system default input.
    Default,
    /// Use this device (the built-in mic) instead of a Bluetooth default input.
    BuiltIn(String),
}

/// Pure policy: opening a Bluetooth headset's microphone flips the headset from A2DP to the
/// call profile (HFP: 16/24 kHz, playback degraded, some players lose their stream). So when
/// the user did not pick a mic, the default input is Bluetooth and no other app is already
/// recording from it, Lowk listens to the built-in mic instead and never touches the headset.
pub fn choose_auto_mic(
    default_is_bluetooth: bool,
    other_app_records: bool,
    builtin: Option<&str>,
) -> MicChoice {
    match builtin {
        Some(b) if default_is_bluetooth && !other_app_records => MicChoice::BuiltIn(b.to_string()),
        _ => MicChoice::Default,
    }
}

fn is_bluetooth(d: &ca::Device) -> bool {
    matches!(d.transport_type(), Ok(t) if t == ca::DeviceTransportType::BLUETOOTH || t == ca::DeviceTransportType::BLUETOOTH_LE)
}

/// Built-in microphone UID, if the Mac has one.
fn builtin_input_uid() -> Option<String> {
    ca::System::devices().ok()?.into_iter().find_map(|d| {
        let builtin = matches!(d.transport_type(), Ok(t) if t == ca::DeviceTransportType::BUILT_IN);
        if builtin && has_input(&d) {
            d.uid().ok().map(|s| s.to_string())
        } else {
            None
        }
    })
}

/// Another process (not us) is reading audio input right now.
fn other_app_records() -> bool {
    let me = std::process::id() as i32;
    ca::Process::list()
        .map(|l| {
            l.iter()
                .any(|p| p.is_running_input().unwrap_or(false) && p.pid().map(|x| x != me).unwrap_or(true))
        })
        .unwrap_or(false)
}

/// Mic to use when the user left the choice on "default".
pub fn auto_mic_uid() -> MicChoice {
    let Ok(def) = ca::System::default_input_device() else {
        return MicChoice::Default;
    };
    choose_auto_mic(
        is_bluetooth(&def),
        other_app_records(),
        builtin_input_uid().as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_default_input_is_replaced_by_the_builtin_mic() {
        assert_eq!(
            choose_auto_mic(true, false, Some("bi")),
            MicChoice::BuiltIn("bi".into())
        );
        // headset mic already in use by another app (HFP is on anyway), no built-in, or a wired default
        assert_eq!(choose_auto_mic(true, true, Some("bi")), MicChoice::Default);
        assert_eq!(choose_auto_mic(true, false, None), MicChoice::Default);
        assert_eq!(choose_auto_mic(false, false, Some("bi")), MicChoice::Default);
    }
}
