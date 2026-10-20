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
