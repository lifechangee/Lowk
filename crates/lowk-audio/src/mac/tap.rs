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
    let first_mono = deliver_mono.saturating_sub(deliver_host.saturating_sub(cap_host));

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
    pub rate: f64,
    pub out_uid: String,
}

impl TapSession {
    /// Format rate the tap delivers right now (changes when the output device renegotiates).
    pub fn current_rate(&self) -> Option<f64> {
        self._tap.asbd().ok().map(|a| a.sample_rate)
    }
}

pub fn start_tap(target: &TapTarget, ring: RingIn) -> Result<TapSession> {
    let nums: Vec<arc::R<ns::Number>> = match target {
        TapTarget::System(ex) => ex.iter().copied().map(ns::Number::with_u32).collect(),
        TapTarget::Procs(p) => p.iter().copied().map(ns::Number::with_u32).collect(),
    };
    let refs: Vec<&ns::Number> = nums.iter().map(|n| &**n).collect();
    let arr = ns::Array::from_slice(&refs);
    let mut desc = match target {
        TapTarget::System(_) => ca::TapDesc::with_mono_global_tap_excluding_processes(&arr),
        TapTarget::Procs(p) => {
            if p.is_empty() {
                return Err(AppError::new(ErrorCode::AudioTargetLost).with_detail("no audio process objects"));
            }
            ca::TapDesc::with_mono_mixdown_of_processes(&arr)
        }
    };
    desc.set_private(true);
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
    let mut ctx = Box::new(Ctx {
        ring,
        rate: asbd.sample_rate as u32,
    });
    let pid = agg
        .create_io_proc_id(io_proc as ca::DeviceIoProc<1, 1, Ctx>, Some(&mut *ctx))
        .map_err(|e| err("tap IOProc", e))?;
    let started = ca::device_start(agg, Some(pid)).map_err(|e| err("tap start", e))?;
    tracing::info!(
        rate = asbd.sample_rate,
        channels = asbd.channels_per_frame,
        ?target,
        "tap started"
    );
    Ok(TapSession {
        _started: started,
        _tap: tap,
        _ctx: ctx,
        rate: asbd.sample_rate,
        out_uid: out_uid.to_string(),
    })
}
