//! Windows permission backends. Only the microphone privacy toggle exists.
//! Compiles, unverified on real hardware.
use lowk_types::{PermKind, PermState, Result};
use std::os::windows::process::CommandExt;
use windows::core::w;
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `Some("Allow" | "Deny")` of the CapabilityAccessManager consent value.
fn consent(root: HKEY) -> Option<String> {
    let mut buf = [0u16; 32];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: buffer and length are valid for the duration of the call.
    let r = unsafe {
        RegGetValueW(
            root,
            w!(r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone"),
            w!("Value"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if r.is_err() {
        return None;
    }
    let n = (len as usize / 2).min(buf.len());
    Some(
        String::from_utf16_lossy(&buf[..n])
            .trim_end_matches('\0')
            .to_string(),
    )
}

fn mic_status() -> PermState {
    // The machine-wide switch wins over the per-user one.
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        if let Some(v) = consent(root) {
            if v.eq_ignore_ascii_case("Deny") {
                return PermState::Denied;
            }
        }
    }
    // "Allow" or value absent (desktop apps are allowed by default).
    PermState::Granted
}

pub fn status(k: PermKind) -> PermState {
    match k {
        PermKind::Microphone => mic_status(),
        _ => PermState::NotApplicable,
    }
}

pub fn request(k: PermKind) -> Result<PermState> {
    // Desktop apps get no OS prompt on Windows; the user flips the toggle.
    Ok(status(k))
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}
