//! Core Audio capture sessions: HAL input (mic) and process taps behind a private aggregate
//! device. Ported from `spikes/audio/src/mac.rs`; the IOProc only mixes to mono and pushes
//! into the SPSC ring (stack scratch, no allocation, no locks).
use super::devices;
use crate::pipeline::RingIn;
use cidre::{arc, cat, cf, core_audio as ca, ns, os};
use lowk_types::{now_mono_ns, AppError, ErrorCode, Result};

fn err(what: &str, e: impl std::fmt::Debug) -> AppError {
    AppError::new(ErrorCode::AudioDevice).with_detail(format!("{what}: {e:?}"))
}

struct Ctx {
    ring: RingIn,
    rate: u32,
}

const CHUNK: usize = 512;
const MAX_BUFS: usize = 8;

/// Capture time of a buffer's first sample on the process monotonic clock, from the HAL host
/// times. The HAL lag (`deliver - capture`) is only trusted up to [`MAX_HAL_LAG_NS`]: a stale or
/// garbage `host_time` (device start, aggregate re-clock) used to push every frame to ~0, which
/// stamped lines `[00:00:00]` for the whole call (S6).
pub(crate) fn capture_start_mono(deliver_mono: u64, deliver_host: u64, cap_host: u64) -> u64 {
    let lag = deliver_host.saturating_sub(cap_host);
    let lag = if lag > MAX_HAL_LAG_NS { 0 } else { lag };
    deliver_mono.saturating_sub(lag)
}
const MAX_HAL_LAG_NS: u64 = 1_000_000_000;

extern "C" fn io_proc(
    _dev: ca::Device,
    _now: &cat::AudioTimeStamp,
    input: &cat::AudioBufList<1>,
    input_time: &cat::AudioTimeStamp,
    _out: &mut cat::AudioBufList<1>,
    _out_time: &cat::AudioTimeStamp,
    ctx: Option<&mut Ctx>,
) -> os::Status {
    let Some(ctx) = ctx else { return Default::default() };
    let nb = (input.number_buffers as usize).min(MAX_BUFS);
    if nb == 0 {
        return Default::default();
    }
    // Capture time of the first sample on our monotonic clock.
    let deliver_host = ca::host_time_to_nanos(ca::current_host_time());
    let deliver_mono = now_mono_ns();
    let cap_host = if input_time.host_time != 0 {
        ca::host_time_to_nanos(input_time.host_time)
    } else {
        deliver_host
    };
    let first_mono = capture_start_mono(deliver_mono, deliver_host, cap_host);

    // `buffers` is a flexible array: walk it by pointer.
    // SAFETY: Core Audio guarantees `number_buffers` valid AudioBuffer entries after the header.
    let base = unsafe {
        (input as *const _ as *const u8).add(std::mem::offset_of!(cat::AudioBufList<1>, buffers))
            as *const cat::AudioBuf
    };
    let mut bufs: [(&[f32], usize); MAX_BUFS] = [(&[], 1); MAX_BUFS];
    let mut frames = usize::MAX;
    let mut used = 0;
    for (i, slot) in bufs.iter_mut().enumerate().take(nb) {
        // SAFETY: see above.
        let b = unsafe { &*base.add(i) };
        let nch = (b.number_channels as usize).max(1);
        if b.data.is_null() || b.data_bytes_size < 4 {
            continue;
        }
        let n = b.data_bytes_size as usize / 4 / nch;
        // SAFETY: the HAL provides `data_bytes_size` bytes of f32 samples at `data`.
        let s = unsafe { std::slice::from_raw_parts(b.data as *const f32, n * nch) };
        *slot = (s, nch);
        frames = frames.min(n);
        used += 1;
    }
    if used == 0 || frames == 0 || frames == usize::MAX {
        return Default::default();
    }
    let inv = 1.0 / used as f32;
    let mut scratch = [0f32; CHUNK];
    let mut done = 0;
    while done < frames {
        let len = (frames - done).min(CHUNK);
        for (k, out) in scratch[..len].iter_mut().enumerate() {
            let f = done + k;
            let mut acc = 0.0;
            for (s, nch) in bufs.iter().take(nb).filter(|(s, _)| !s.is_empty()) {
                let mut a = 0.0;
                for c in 0..*nch {
                    a += s[f * nch + c];
                }
                acc += a / *nch as f32;
            }
            *out = acc * inv;
        }
        // a buffer cannot end after the moment it was delivered
        let end = (first_mono + ((done + len) as u64 * 1_000_000_000) / ctx.rate as u64).min(deliver_mono);
        ctx.ring.push(&scratch[..len], ctx.rate, end);
        done += len;
    }
    Default::default()
}

extern "C" {
    // CoreAudio.framework; cidre does not wrap it.
    fn AudioDeviceDestroyIOProcID(device: ca::Device, proc_id: ca::DeviceIoProcId) -> i32;
}

// ---------------------------------------------------------------- mic

pub struct MicSession {
    started: Option<ca::hardware::StartedDevice<ca::Device>>,
    dev: ca::Device,
    proc_id: ca::DeviceIoProcId,
    _ctx: Box<Ctx>,
    pub uid: String,
    pub name: String,
    pub rate: f64,
}

impl Drop for MicSession {
    fn drop(&mut self) {
        // Stop first (blocks until the IOProc has returned), then unregister it.
        drop(self.started.take());
        // SAFETY: device and proc id come from `create_io_proc_id` below and are destroyed once.
        unsafe { AudioDeviceDestroyIOProcID(ca::Device(self.dev.0), self.proc_id) };
    }
}

/// `uid = None` -> current default input device.
pub fn start_mic(uid: Option<&str>, ring: RingIn) -> Result<MicSession> {
    let dev = match uid {
        Some(u) => devices::device_by_uid(u)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail(format!("input device {u}")))?,
        None => ca::System::default_input_device().map_err(|e| err("no default input", e))?,
    };
    let rate = dev.actual_sample_rate().map_err(|e| err("input rate", e))?;
    let name = dev.name().map(|s| s.to_string()).unwrap_or_default();
    let uid = dev.uid().map(|s| s.to_string()).unwrap_or_default();
    let mut ctx = Box::new(Ctx {
        ring,
        rate: rate as u32,
    });
    let proc_id = dev
        .create_io_proc_id(io_proc as ca::DeviceIoProc<1, 1, Ctx>, Some(&mut *ctx))
        .map_err(|e| err("mic IOProc", e))?;
    let d2 = ca::Device(ca::Obj(dev.0 .0));
    let started = ca::device_start(dev, Some(proc_id)).map_err(|e| {
        // SAFETY: proc id was just created on this device and never started.
        unsafe { AudioDeviceDestroyIOProcID(ca::Device(ca::Obj(d2.0 .0)), proc_id) };
        err("mic start", e)
    })?;
    Ok(MicSession {
        started: Some(started),
        dev: d2,
        proc_id,
        _ctx: ctx,
        uid,
        name,
        rate,
    })
}

// ---------------------------------------------------------------- tap

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapTarget {
    /// Everything except these process objects (our own process tree).
    System(Vec<u32>),
    /// Mono mixdown of these Core Audio process objects.
    Procs(Vec<u32>),
}

pub struct TapSession {
    // Drop order matters: stop the device (and destroy the aggregate) first, then the tap,
    // then the context the IOProc pointed at.
    _started: ca::hardware::StartedDevice<ca::AggregateDevice>,
    _tap: ca::TapGuard,
    _ctx: Box<Ctx>,
    /// Rate the IOProc really delivers: the aggregate device clock, NOT the tap format.
    pub rate: f64,
    pub out_uid: String,
    agg_obj: u32,
}

impl TapSession {
    /// Rate the aggregate delivers right now (changes when the output device renegotiates, e.g.
    /// Bluetooth A2DP 48 kHz <-> HFP 24/16 kHz when a microphone opens).
    pub fn current_rate(&self) -> Option<f64> {
        ca::Device(ca::Obj(self.agg_obj))
            .actual_sample_rate()
            .ok()
            .filter(|r| *r > 0.0)
    }
}

/// The rate of the buffers the aggregate IOProc delivers. The tap's own ASBD keeps saying the
/// output device's nominal rate (48 kHz) while a Bluetooth headset in HFP runs the aggregate at
/// 24/16 kHz; trusting the tap format played everything at 2x speed (AirPods bug).
pub(crate) fn delivered_rate(agg_rate: Option<f64>, tap_rate: f64) -> f64 {
    match agg_rate {
        Some(r) if r.is_finite() && r >= 8_000.0 => r,
        _ => tap_rate,
    }
}

/// Process object ids safe to hand to `CATapDescription`: drops `kAudioObjectUnknown` (0; the
/// PID translation returns it with a *success* status for a process without an audio object),
/// all-ones sentinels and duplicates, and, when the live process list is known, ids that no
/// longer exist (a stale id makes `AudioHardwareCreateProcessTap` fail with `!obj`).
pub fn sanitize_ids(raw: &[u32], live: Option<&[u32]>) -> Vec<u32> {
    let mut v: Vec<u32> = raw
        .iter()
        .copied()
        .filter(|&i| i != 0 && i != u32::MAX && live.is_none_or(|l| l.contains(&i)))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Tap description for a target: private and explicitly CATapUnmuted so capture never changes
/// what the user hears (whole-system and per-app, any output device).
pub(crate) fn build_desc(target: &TapTarget, arr: &ns::Array<ns::Number>) -> arc::R<ca::TapDesc> {
    let mut desc = match target {
        TapTarget::System(_) => ca::TapDesc::with_mono_global_tap_excluding_processes(arr),
        TapTarget::Procs(_) => ca::TapDesc::with_mono_mixdown_of_processes(arr),
    };
    desc.set_private(true);
    desc.set_mute_behavior(ca::TapMuteBehavior::Unmuted);
    desc
}

pub fn start_tap(target: &TapTarget, ring: RingIn) -> Result<TapSession> {
    let live: Option<Vec<u32>> = ca::Process::list()
        .ok()
        .map(|l| l.iter().map(|p| p.0 .0).collect());
    let target = &match target {
        TapTarget::System(ex) => TapTarget::System(sanitize_ids(ex, live.as_deref())),
        TapTarget::Procs(p) => TapTarget::Procs(sanitize_ids(p, live.as_deref())),
    };
    let nums: Vec<arc::R<ns::Number>> = match target {
        TapTarget::System(ex) => ex.iter().copied().map(ns::Number::with_u32).collect(),
        TapTarget::Procs(p) => p.iter().copied().map(ns::Number::with_u32).collect(),
    };
    let refs: Vec<&ns::Number> = nums.iter().map(|n| &**n).collect();
    let arr = ns::Array::from_slice(&refs);
    if matches!(target, TapTarget::Procs(p) if p.is_empty()) {
        return Err(AppError::new(ErrorCode::AudioTargetLost).with_detail("no audio process objects"));
    }
    let desc = build_desc(target, &arr);
    let tap = desc
        .create_process_tap()
        .map_err(|e| err("AudioHardwareCreateProcessTap", e))?;
    let asbd = tap.asbd().map_err(|e| err("tap format", e))?;

    let out_dev = ca::System::default_output_device().map_err(|e| err("default output", e))?;
    let out_uid = out_dev.uid().map_err(|e| err("output uid", e))?;
    let sub_device =
        cf::DictionaryOf::with_keys_values(&[ca::sub_device_keys::uid()], &[out_uid.as_type_ref()]);
    let sub_tap = cf::DictionaryOf::with_keys_values(
        &[ca::sub_device_keys::uid()],
        &[tap.uid().map_err(|e| err("tap uid", e))?.as_type_ref()],
    );
    use ca::aggregate_device_keys as k;
    let dict = cf::DictionaryOf::with_keys_values(
        &[
            k::is_private(),
            k::is_stacked(),
            k::tap_auto_start(),
            k::name(),
            k::main_sub_device(),
            k::uid(),
            k::sub_device_list(),
            k::tap_list(),
        ],
        &[
            cf::Boolean::value_true().as_type_ref(),
            cf::Boolean::value_false(),
            cf::Boolean::value_true(),
            cf::str!(c"LowkTap"),
            &out_uid,
            &cf::Uuid::new().to_cf_string(),
            &cf::ArrayOf::from_slice(&[sub_device.as_ref()]),
            &cf::ArrayOf::from_slice(&[sub_tap.as_ref()]),
        ],
    );
    let agg = ca::AggregateDevice::with_desc(&dict).map_err(|e| err("create aggregate", e))?;
    let agg_obj = agg.0 .0;
    let rate = delivered_rate(agg.actual_sample_rate().ok(), asbd.sample_rate);
    if (rate - asbd.sample_rate).abs() > 1.0 {
        tracing::info!(
            tap_rate = asbd.sample_rate,
            agg_rate = rate,
            "aggregate runs at another rate than the tap format"
        );
    }
    let mut ctx = Box::new(Ctx {
        ring,
        rate: rate as u32,
    });
    let pid = agg
        .create_io_proc_id(io_proc as ca::DeviceIoProc<1, 1, Ctx>, Some(&mut *ctx))
        .map_err(|e| err("tap IOProc", e))?;
    let started = ca::device_start(agg, Some(pid)).map_err(|e| err("tap start", e))?;
    tracing::info!(rate, channels = asbd.channels_per_frame, ?target, "tap started");
    Ok(TapSession {
        _started: started,
        _tap: tap,
        _ctx: ctx,
        rate,
        out_uid: out_uid.to_string(),
        agg_obj,
    })
}

#[cfg(test)]
mod tests {
    use super::{delivered_rate, sanitize_ids};

    #[test]
    fn delivered_rate_follows_the_aggregate_not_the_tap_format() {
        // AirPods in HFP: tap says 48 kHz, aggregate runs at 24 kHz
        assert_eq!(delivered_rate(Some(24_000.0), 48_000.0), 24_000.0);
        assert_eq!(delivered_rate(Some(16_000.0), 48_000.0), 16_000.0);
        assert_eq!(delivered_rate(Some(44_100.0), 44_100.0), 44_100.0);
        // unreadable / nonsense -> tap format
        assert_eq!(delivered_rate(None, 48_000.0), 48_000.0);
        assert_eq!(delivered_rate(Some(0.0), 48_000.0), 48_000.0);
        assert_eq!(delivered_rate(Some(f64::NAN), 44_100.0), 44_100.0);
    }

    #[test]
    fn invalid_ids_are_filtered() {
        assert!(sanitize_ids(&[0, u32::MAX], None).is_empty());
        assert_eq!(sanitize_ids(&[7, 0, 7, 3], None), vec![3, 7]);
    }

    #[test]
    fn stale_ids_are_filtered_against_live_list() {
        assert_eq!(sanitize_ids(&[3, 9, 12], Some(&[3, 12, 40])), vec![3, 12]);
        assert!(sanitize_ids(&[9], Some(&[])).is_empty());
    }
}

#[cfg(test)]
mod mute_tests {
    use super::*;

    #[test]
    fn tap_description_is_unmuted_for_every_target_kind() {
        let arr = ns::Array::<ns::Number>::new();
        for t in [TapTarget::System(vec![]), TapTarget::Procs(vec![])] {
            let d = build_desc(&t, &arr);
            assert_eq!(d.mute_behavior(), ca::TapMuteBehavior::Unmuted, "{t:?}");
            assert!(d.is_private());
        }
    }
}

/// Audible QA helper (driven by tools/audio/no-mute.sh, opt-in): holds a real tap open.
#[cfg(test)]
mod audible_hold {
    use super::*;

    #[test]
    #[ignore = "audible QA: run through tools/audio/no-mute.sh"]
    fn hold_tap_open() {
        let secs: u64 = std::env::var("LOWK_HOLD_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6);
        let target = match std::env::var("LOWK_HOLD_PIDS") {
            Ok(p) if !p.is_empty() => {
                let ids: Vec<u32> = p.split(',').filter_map(|x| x.parse().ok()).collect();
                TapTarget::Procs(ids)
            }
            _ => TapTarget::System(vec![]),
        };
        let (ring, _rx) = RingIn::new(Default::default(), 48_000);
        // LOWK_HOLD_MIC=default: system default input (old behaviour); =auto: the app's policy.
        let _mic = std::env::var("LOWK_HOLD_MIC").ok().map(|m| {
            let uid = match (m.as_str(), devices::auto_mic_uid()) {
                ("auto", devices::MicChoice::BuiltIn(b)) => Some(b),
                _ => None,
            };
            start_mic(uid.as_deref(), RingIn::new(Default::default(), 48_000).0).expect("mic")
        });
        let _t = start_tap(&target, ring).expect("tap");
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
}

#[cfg(test)]
mod s6_tests {
    use super::*;

    #[test]
    fn s6_capture_time_ignores_garbage_host_times() {
        // normal: buffer captured 20 ms before delivery
        assert_eq!(
            capture_start_mono(10_000_000_000, 500_000_000, 480_000_000),
            9_980_000_000
        );
        // stale host time (device start, hours ago): never collapse to ~0
        assert_eq!(
            capture_start_mono(3_000_000_000, 90_000_000_000_000, 1),
            3_000_000_000
        );
        // capture "in the future": no negative lag
        assert_eq!(capture_start_mono(5_000_000_000, 100, 200), 5_000_000_000);
    }
}
