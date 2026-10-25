//! OS proxy settings glue. The parsing lives in `parse` (pure, tested on every platform).
use super::Config;

#[cfg(windows)]
pub fn read() -> Option<Config> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    const KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings");
    let dword = |name: PCWSTR| -> Option<u32> {
        let mut v = 0u32;
        let mut n = 4u32;
        // SAFETY: `v` and `n` outlive the call; the buffer size matches.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                KEY,
                name,
                RRF_RT_REG_DWORD,
                None,
                Some(&mut v as *mut u32 as *mut _),
                Some(&mut n),
            )
        };
        r.is_ok().then_some(v)
    };
    let string = |name: PCWSTR| -> Option<String> {
        let mut n = 0u32;
        // SAFETY: first call only asks for the size.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut n),
            )
        };
        if r.is_err() || n < 2 {
            return None;
        }
        let mut buf = vec![0u16; (n as usize).div_ceil(2)];
        let mut n2 = (buf.len() * 2) as u32;
        // SAFETY: `buf` holds `n2` bytes.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut n2),
            )
        };
        if r.is_err() {
            return None;
        }
        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    };
    let enable = dword(w!("ProxyEnable")).unwrap_or(0) != 0;
    let server = string(w!("ProxyServer")).unwrap_or_default();
    let over = string(w!("ProxyOverride")).unwrap_or_default();
    super::parse_windows(enable, &server, &over)
}

#[cfg(target_os = "macos")]
pub fn read() -> Option<Config> {
    use super::MacVal;
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    let dict = SCDynamicStoreBuilder::new("lowk-proxy").build()?.get_proxies()?;
    let mut map = std::collections::HashMap::new();
    let (keys, values) = dict.get_keys_and_values();
    for (k, v) in keys.into_iter().zip(values) {
        // SAFETY: keys of this dictionary are CFStrings; values are wrapped under the get rule.
        let key = unsafe { CFString::wrap_under_get_rule(k as *const _) }.to_string();
        let val = unsafe { CFType::wrap_under_get_rule(v as *const _) };
        if let Some(n) = val.downcast::<CFNumber>().and_then(|n| n.to_i64()) {
            map.insert(key, MacVal::Num(n));
        } else if let Some(s) = val.downcast::<CFString>() {
            map.insert(key, MacVal::Str(s.to_string()));
        } else if let Some(a) = val.downcast::<CFArray>() {
            let list = a
                .iter()
                .filter_map(|e| {
                    // SAFETY: elements are borrowed under the get rule.
                    unsafe { CFType::wrap_under_get_rule(*e as *const _) }
                        .downcast::<CFString>()
                        .map(|s| s.to_string())
                })
                .collect();
            map.insert(key, MacVal::List(list));
        }
    }
    super::parse_macos(&map)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn read() -> Option<Config> {
    None
}
