//! Hardware probe and the static local-vs-cloud recommendation (ARCHITECTURE 6.4).
//! The recommendation is a pre-filter; the 5-10 s benchmark has the last word.
use lowk_types::{GpuInfo, HwInfo, HwReport, Os, RecReason, Recommendation, SttTier};
use sysinfo::{Disks, System};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

pub fn probe() -> HwInfo {
    probe_report().info
}

pub fn probe_report() -> HwReport {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let chip = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    let os = Os::current();
    let ram_gb = (sys.total_memory() as f64 / GIB) as f32;
    let logical = sys.cpus().len() as u32;
    let physical = System::physical_core_count().map(|n| n as u32).unwrap_or(logical);
    let mut gpus = os_impl::gpus(&chip, sys.total_memory());
    gpus.sort_by_key(|g| std::cmp::Reverse(g.vram_mb));
    let info = HwInfo {
        os,
        os_version: System::long_os_version()
            .or_else(System::os_version)
            .unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
        chip,
        ram_gb,
        gpus,
        cpu_features: cpu_features(),
        free_disk_gb: free_disk_gb(),
        on_battery: os_impl::on_battery(),
    };
    HwReport {
        apple_silicon_gen: apple_silicon_gen(&info.chip),
        info,
        physical_cores: physical,
        logical_cores: logical,
    }
}

/// "Apple M4 Pro" -> Some(4); "Intel(R) Core(TM) i5" -> None.
pub fn apple_silicon_gen(chip: &str) -> Option<u8> {
    let rest = chip.strip_prefix("Apple M")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[allow(unused_mut)]
fn cpu_features() -> Vec<String> {
    let mut f: Vec<String> = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        macro_rules! chk { ($($n:tt),*) => { $( if std::is_x86_feature_detected!($n) { f.push($n.to_string()); } )* } }
        chk!("sse4.2", "avx", "avx2", "fma", "avx512f", "avx512bw");
    }
    #[cfg(target_arch = "aarch64")]
    {
        macro_rules! chk { ($($n:tt),*) => { $( if std::arch::is_aarch64_feature_detected!($n) { f.push($n.to_string()); } )* } }
        chk!("neon", "dotprod", "fp16", "i8mm");
    }
    f
}

fn free_disk_gb() -> f32 {
    let target = crate::dirs::app_data()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| target.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        // On macOS the data volume is mounted separately; fall back to "/".
        .or_else(|| disks.list().iter().find(|d| d.mount_point().as_os_str() == "/"))
        .map(|d| (d.available_space() as f64 / GIB) as f32)
        .unwrap_or(0.0)
}

/// Static recommendation. Tested against the two project machines.
pub fn recommend(r: &HwReport) -> Recommendation {
    use RecReason::*;
    use SttTier::*;
    let rec = |tier, reason| Recommendation { tier, reason };
    let ram = r.info.ram_gb;
    if ram < 7.5 {
        return rec(Cloud, LowRam);
    }
    if r.info.os == Os::Macos {
        return match r.apple_silicon_gen {
            None => rec(Cloud, IntelMac),
            Some(g) if g >= 2 && ram >= 15.5 => rec(LocalHigh, AppleSiliconCapable),
            Some(_) => rec(LocalLight, AppleSiliconLimited),
        };
    }
    // Windows / Linux: best discrete GPU decides (NVIDIA CUDA, others via DirectML).
    let dedicated = r
        .info
        .gpus
        .iter()
        .filter(|g| g.vendor != "Intel" && g.vendor != "Apple")
        .map(|g| g.vram_mb)
        .max()
        .unwrap_or(0);
    if dedicated >= 6 * 1024 && ram >= 11.5 {
        return rec(LocalHigh, DiscreteGpu);
    }
    if dedicated >= 3 * 1024 {
        return rec(LocalLight, DiscreteGpuSmall);
    }
    let avx2 = r.info.cpu_features.iter().any(|f| f == "avx2");
    if r.physical_cores >= 8 && ram >= 15.5 && avx2 {
        return rec(LocalLight, CpuOnlyCapable);
    }
    rec(Cloud, WeakCpu)
}

#[cfg(target_os = "macos")]
mod os_impl {
    use super::*;
    use std::ffi::{c_char, c_void};

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> *const c_void;
        fn IOPSGetProvidingPowerSourceType(snapshot: *const c_void) -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(s: *const c_void, buf: *mut c_char, size: isize, enc: u32) -> bool;
        fn CFRelease(o: *const c_void);
    }

    pub fn on_battery() -> bool {
        // SAFETY: Copy-rule snapshot released below; the type string follows the
        // Get rule (borrowed from the snapshot) and is copied before release.
        unsafe {
            let info = IOPSCopyPowerSourcesInfo();
            if info.is_null() {
                return false;
            }
            let ty = IOPSGetProvidingPowerSourceType(info);
            let mut buf = [0 as c_char; 64];
            let ok =
                !ty.is_null() && CFStringGetCString(ty, buf.as_mut_ptr(), buf.len() as isize, 0x0800_0100);
            let s = if ok {
                std::ffi::CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            CFRelease(info);
            s == "Battery Power"
        }
    }

    /// Apple GPUs share system memory; report the unified pool as VRAM.
    pub fn gpus(chip: &str, total_mem: u64) -> Vec<GpuInfo> {
        if chip.starts_with("Apple") {
            vec![GpuInfo {
                vendor: "Apple".into(),
                name: chip.to_string(),
                vram_mb: (total_mem / (1024 * 1024)).min(u32::MAX as u64) as u32,
            }]
        } else {
            vec![]
        }
    }
}

#[cfg(windows)]
mod os_impl {
    use super::*;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    pub fn on_battery() -> bool {
        let mut st = SYSTEM_POWER_STATUS::default();
        // SAFETY: out-pointer to a valid struct.
        unsafe { GetSystemPowerStatus(&mut st).is_ok() && st.ACLineStatus == 0 }
    }

    fn vendor(id: u32) -> &'static str {
        match id {
            0x10DE => "NVIDIA",
            0x1002 | 0x1022 => "AMD",
            0x8086 => "Intel",
            0x1414 => "Microsoft",
            _ => "Unknown",
        }
    }

    /// Adapters via DXGI; `DedicatedVideoMemory` is true VRAM for discrete GPUs.
    pub fn gpus(_chip: &str, _total_mem: u64) -> Vec<GpuInfo> {
        let mut out = Vec::new();
        // SAFETY: plain COM calls on interfaces we own.
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                return out;
            };
            let mut i = 0;
            while let Ok(a) = factory.EnumAdapters1(i) {
                i += 1;
                let Ok(d) = a.GetDesc1() else { continue };
                if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                    continue;
                }
                let len = d
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(d.Description.len());
                out.push(GpuInfo {
                    vendor: vendor(d.VendorId).to_string(),
                    name: String::from_utf16_lossy(&d.Description[..len]),
                    vram_mb: (d.DedicatedVideoMemory / (1024 * 1024)).min(u32::MAX as usize) as u32,
                });
            }
        }
        out
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod os_impl {
    use super::*;
    pub fn on_battery() -> bool {
        false
    }
    pub fn gpus(_chip: &str, _total_mem: u64) -> Vec<GpuInfo> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feats(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Test Mac: MacBook M4, 24 GB.
    fn m4_mac() -> HwReport {
        HwReport {
            info: HwInfo {
                os: Os::Macos,
                os_version: "macOS 15".into(),
                arch: "aarch64".into(),
                chip: "Apple M4".into(),
                ram_gb: 24.0,
                gpus: vec![GpuInfo {
                    vendor: "Apple".into(),
                    name: "Apple M4".into(),
                    vram_mb: 24576,
                }],
                cpu_features: feats(&["neon"]),
                free_disk_gb: 200.0,
                on_battery: false,
            },
            physical_cores: 10,
            logical_cores: 10,
            apple_silicon_gen: Some(4),
        }
    }

    /// Test PC: i5-10400F, 16 GB, RTX 3060 12 GB.
    fn rtx_pc() -> HwReport {
        HwReport {
            info: HwInfo {
                os: Os::Windows,
                os_version: "Windows 11".into(),
                arch: "x86_64".into(),
                chip: "Intel(R) Core(TM) i5-10400F CPU @ 2.90GHz".into(),
                ram_gb: 15.9,
                gpus: vec![GpuInfo {
                    vendor: "NVIDIA".into(),
                    name: "GeForce RTX 3060".into(),
                    vram_mb: 12288,
                }],
                cpu_features: feats(&["avx", "avx2", "fma"]),
                free_disk_gb: 300.0,
                on_battery: false,
            },
            physical_cores: 6,
            logical_cores: 12,
            apple_silicon_gen: None,
        }
    }

    #[test]
    fn chip_gen() {
        assert_eq!(apple_silicon_gen("Apple M4"), Some(4));
        assert_eq!(apple_silicon_gen("Apple M1 Pro"), Some(1));
        assert_eq!(apple_silicon_gen("Apple M12 Max"), Some(12));
        assert_eq!(apple_silicon_gen("Intel(R) Core(TM) i5"), None);
        assert_eq!(apple_silicon_gen("Apple A18"), None);
    }

    #[test]
    fn test_mac_runs_local_high() {
        let r = recommend(&m4_mac());
        assert_eq!(
            (r.tier, r.reason),
            (SttTier::LocalHigh, RecReason::AppleSiliconCapable)
        );
    }

    #[test]
    fn test_pc_runs_local_high_on_gpu() {
        let r = recommend(&rtx_pc());
        assert_eq!((r.tier, r.reason), (SttTier::LocalHigh, RecReason::DiscreteGpu));
    }

    #[test]
    fn test_pc_without_gpu_goes_cloud() {
        let mut p = rtx_pc();
        p.info.gpus.clear();
        assert_eq!(recommend(&p).tier, SttTier::Cloud);
    }

    #[test]
    fn small_and_old_machines() {
        let mut m = m4_mac();
        m.info.ram_gb = 8.0;
        assert_eq!(recommend(&m).tier, SttTier::LocalLight);
        m.info.ram_gb = 4.0;
        assert_eq!(recommend(&m).reason, RecReason::LowRam);
        let mut intel = m4_mac();
        intel.apple_silicon_gen = None;
        assert_eq!(recommend(&intel).reason, RecReason::IntelMac);
        let mut gtx = rtx_pc();
        gtx.info.gpus[0].vram_mb = 4096;
        assert_eq!(recommend(&gtx).tier, SttTier::LocalLight);
        let mut igpu = rtx_pc();
        igpu.info.gpus = vec![GpuInfo {
            vendor: "Intel".into(),
            name: "UHD".into(),
            vram_mb: 1024,
        }];
        assert_eq!(recommend(&igpu).tier, SttTier::Cloud);
    }

    #[test]
    fn probe_is_sane() {
        let r = probe_report();
        assert!(r.info.ram_gb > 1.0);
        assert!(r.logical_cores >= 1);
        assert_eq!(r.info.os, Os::current());
        assert!(!r.info.arch.is_empty());
    }
}
