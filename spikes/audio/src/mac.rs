//! macOS backend: Core Audio process taps (via cidre) + HAL mic input.
use crate::dsp::Chunk;
use crate::clock::now_ns;
use anyhow::{anyhow, bail, Result};
use ca::aggregate_device_keys as agg_keys;
use ca::sub_device_keys as sub_keys;
use cidre::{arc, cat, cf, core_audio as ca, ns, os};
use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

// ---------------------------------------------------------------- process enumeration

#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub obj: u32,
    pub pid: i32,
    pub bundle_id: String,
    pub path: String,
    pub running_output: bool,
    pub running_input: bool,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub key: String,
    pub name: String,
    pub bundle_id: String,
    pub members: Vec<ProcInfo>,
}
impl Group {
    pub fn any_output(&self) -> bool { self.members.iter().any(|m| m.running_output) }
}

fn pid_path(pid: i32) -> String {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr() as *mut _, buf.len() as u32) };
    if n <= 0 { return String::new(); }
    String::from_utf8_lossy(&buf[..n as usize]).into_owned()
}

/// Outermost `.app` bundle containing this executable path.
fn outer_app(path: &str) -> Option<String> {
    let i = path.find(".app/")?;
    Some(path[..i + 4].to_string())
}

fn app_bundle_id(app: &str) -> Option<String> {
    let v = plist::Value::from_file(format!("{app}/Contents/Info.plist")).ok()?;
    v.as_dictionary()?.get("CFBundleIdentifier")?.as_string().map(|s| s.to_string())
}

pub fn list_processes() -> Result<Vec<ProcInfo>> {
    let mut v = vec![];
    for p in ca::System::processes().map_err(|e| anyhow!("process list: {e:?}"))? {
        let pid = p.pid().unwrap_or(-1);
        v.push(ProcInfo {
            obj: p.0 .0,
            pid,
            bundle_id: p.bundle_id().map(|s| s.to_string()).unwrap_or_default(),
            path: pid_path(pid),
            running_output: p.is_running_output().unwrap_or(false),
            running_input: p.is_running_input().unwrap_or(false),
        });
    }
    Ok(v)
}

/// Group helper processes under the owning .app (Chrome Helper -> Chrome etc).
pub fn groups() -> Result<Vec<Group>> {
    let mut m: BTreeMap<String, Group> = BTreeMap::new();
    for p in list_processes()? {
        let (key, name, bid) = match outer_app(&p.path) {
            Some(app) => {
                let name = std::path::Path::new(&app).file_stem().unwrap().to_string_lossy().into_owned();
                let bid = app_bundle_id(&app).unwrap_or_else(|| p.bundle_id.clone());
                (app, name, bid)
            }
            None => {
                let name = std::path::Path::new(&p.path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or("?".into());
                (format!("pid:{}", p.pid), name, p.bundle_id.clone())
            }
        };
        m.entry(key.clone()).or_insert(Group { key, name, bundle_id: bid, members: vec![] }).members.push(p);
    }
    Ok(m.into_values().collect())
}

pub fn cmd_list(all: bool) -> Result<()> {
    let gs = groups()?;
    println!("{:<44} {:<24} {:>5}  {}", "BUNDLE ID", "NAME", "OUT", "PIDS (* = producing output, i = input)");
    for g in gs.iter().filter(|g| all || g.any_output()) {
        let pids: Vec<String> = g.members.iter().map(|m| format!("{}{}{}", m.pid, if m.running_output { "*" } else { "" }, if m.running_input { "i" } else { "" })).collect();
        println!("{:<44} {:<24} {:>5}  {}", if g.bundle_id.is_empty() { "-" } else { &g.bundle_id }, g.name, if g.any_output() { "yes" } else { "no" }, pids.join(","));
    }
    Ok(())
}

pub enum Target { System, Group(Group) }

pub fn resolve_app(spec: &str) -> Result<Group> {
    let gs = groups()?;
    if let Ok(pid) = spec.parse::<i32>() {
        return gs.into_iter().find(|g| g.members.iter().any(|m| m.pid == pid))
            .ok_or_else(|| anyhow!("pid {pid} has no Core Audio process object (has it touched audio yet?)"));
    }
    let s = spec.to_lowercase();
    gs.into_iter().find(|g| g.bundle_id.to_lowercase() == s || g.members.iter().any(|m| m.bundle_id.to_lowercase() == s) || g.name.to_lowercase() == s)
        .ok_or_else(|| anyhow!("no audio process group matching '{spec}' (try `list --all`)"))
}

/// Re-resolve a group's current members (process set changes while capturing).
pub fn refresh_group(key: &str) -> Result<Group> {
    groups()?.into_iter().find(|g| g.key == key).ok_or_else(|| anyhow!("group {key} gone"))
}

pub fn target_active(t: &Target) -> bool {
    match t {
        Target::Group(g) => refresh_group(&g.key).map(|g| g.any_output()).unwrap_or(false),
        Target::System => {
            let me = std::process::id() as i32;
            list_processes().map(|v| v.iter().any(|p| p.pid != me && p.running_output)).unwrap_or(false)
        }
    }
}

// ---------------------------------------------------------------- IOProc plumbing

struct Ctx { tx: Sender<Chunk>, sr: f64 }

extern "C" fn io_proc(
    _dev: ca::Device,
    _now: &cat::AudioTimeStamp,
    input: &cat::AudioBufList<1>,
    input_time: &cat::AudioTimeStamp,
    _out: &mut cat::AudioBufList<1>,
    _out_time: &cat::AudioTimeStamp,
    ctx: Option<&mut Ctx>,
) -> os::Status {
    let deliver = now_ns();
    let Some(ctx) = ctx else { return Default::default() };
    let nb = input.number_buffers as usize;
    if nb == 0 { return Default::default(); }
    // buffers[] is a flexible array: walk it by pointer
    let base = unsafe { (input as *const _ as *const u8).add(std::mem::offset_of!(cat::AudioBufList<1>, buffers)) as *const cat::AudioBuf };
    let mut mono: Vec<f32> = vec![];
    for i in 0..nb {
        let b = unsafe { &*base.add(i) };
        let nch = (b.number_channels as usize).max(1);
        let n = b.data_bytes_size as usize / 4 / nch;
        if b.data.is_null() { continue; }
        let s = unsafe { std::slice::from_raw_parts(b.data as *const f32, n * nch) };
        if mono.is_empty() { mono = vec![0.0; n]; }
        let m = mono.len().min(n);
        for f in 0..m {
            let mut a = 0.0;
            for c in 0..nch { a += s[f * nch + c]; }
            mono[f] += a / nch as f32;
        }
    }
    if nb > 1 { for v in mono.iter_mut() { *v /= nb as f32; } }
    let cap = if input_time.host_time != 0 { ca::host_time_to_nanos(input_time.host_time) } else { deliver };
    let _ = ctx.tx.send(Chunk { samples: mono, sr: ctx.sr, cap_ns: cap, deliver_ns: deliver });
    Default::default()
}

// ---------------------------------------------------------------- mic

pub struct MicSession {
    _started: ca::hardware::StartedDevice<ca::Device>,
    _ctx: Box<Ctx>,
    pub device_name: String,
    pub rate: f64,
}

pub fn start_mic(tx: Sender<Chunk>) -> Result<MicSession> {
    let dev = ca::System::default_input_device().map_err(|e| anyhow!("no default input: {e:?}"))?;
    let rate = dev.actual_sample_rate().map_err(|e| anyhow!("{e:?}"))?;
    let name = dev.name().map(|s| s.to_string()).unwrap_or_default();
    let mut ctx = Box::new(Ctx { tx, sr: rate });
    let pid = dev.create_io_proc_id(io_proc as ca::DeviceIoProc<1, 1, Ctx>, Some(&mut *ctx)).map_err(|e| anyhow!("mic IOProc: {e:?}"))?;
    let started = ca::device_start(dev, Some(pid)).map_err(|e| anyhow!("mic start: {e:?}"))?;
    Ok(MicSession { _started: started, _ctx: ctx, device_name: name, rate })
}

// ---------------------------------------------------------------- tap

pub struct TapSession {
    // drop order: stop device (+destroy aggregate) first, then tap, then ctx
    _started: ca::hardware::StartedDevice<ca::AggregateDevice>,
    _tap: ca::TapGuard,
    _ctx: Box<Ctx>,
    pub members: Vec<u32>,
    pub rate: f64,
}

pub fn start_tap(target: &Target, members: &[u32], tx: Sender<Chunk>) -> Result<TapSession> {
    let own = ca::Process::with_pid(std::process::id() as i32).ok().map(|p| p.0 .0);
    let nums: Vec<arc::R<ns::Number>> = match target {
        Target::System => own.into_iter().map(ns::Number::with_u32).collect(),
        Target::Group(_) => members.iter().copied().map(ns::Number::with_u32).collect(),
    };
    let refs: Vec<&ns::Number> = nums.iter().map(|n| &**n).collect();
    let arr = ns::Array::from_slice(&refs);
    let mut desc = match target {
        Target::System => ca::TapDesc::with_mono_global_tap_excluding_processes(&arr),
        Target::Group(_) => {
            if members.is_empty() { bail!("target has no audio process objects"); }
            ca::TapDesc::with_mono_mixdown_of_processes(&arr)
        }
    };
    desc.set_private(true);
    let tap = desc.create_process_tap().map_err(|e| anyhow!("AudioHardwareCreateProcessTap: {e:?}"))?;
    let asbd = tap.asbd().map_err(|e| anyhow!("tap format: {e:?}"))?;

    let out_dev = ca::System::default_output_device().map_err(|e| anyhow!("{e:?}"))?;
    let out_uid = out_dev.uid().map_err(|e| anyhow!("{e:?}"))?;
    let sub_device = cf::DictionaryOf::with_keys_values(&[sub_keys::uid()], &[out_uid.as_type_ref()]);
    let sub_tap = cf::DictionaryOf::with_keys_values(&[sub_keys::uid()], &[tap.uid().map_err(|e| anyhow!("{e:?}"))?.as_type_ref()]);
    let dict = cf::DictionaryOf::with_keys_values(
        &[agg_keys::is_private(), agg_keys::is_stacked(), agg_keys::tap_auto_start(), agg_keys::name(), agg_keys::main_sub_device(), agg_keys::uid(), agg_keys::sub_device_list(), agg_keys::tap_list()],
        &[
            cf::Boolean::value_true().as_type_ref(),
            cf::Boolean::value_false(),
            cf::Boolean::value_true(),
            cf::str!(c"LowkSpikeTap"),
            &out_uid,
            &cf::Uuid::new().to_cf_string(),
            &cf::ArrayOf::from_slice(&[sub_device.as_ref()]),
            &cf::ArrayOf::from_slice(&[sub_tap.as_ref()]),
        ],
    );
    let agg = ca::AggregateDevice::with_desc(&dict).map_err(|e| anyhow!("create aggregate: {e:?}"))?;
    let mut ctx = Box::new(Ctx { tx, sr: asbd.sample_rate });
    let pid = agg.create_io_proc_id(io_proc as ca::DeviceIoProc<1, 1, Ctx>, Some(&mut *ctx)).map_err(|e| anyhow!("tap IOProc: {e:?}"))?;
    let started = ca::device_start(agg, Some(pid)).map_err(|e| anyhow!("tap start: {e:?}"))?;
    eprintln!("[tap] started: {} ch @ {} Hz, {} process objects, own excluded: {}", asbd.channels_per_frame, asbd.sample_rate, members.len(), matches!(target, Target::System) && own.is_some());
    Ok(TapSession { _started: started, _tap: tap, _ctx: ctx, members: members.to_vec(), rate: asbd.sample_rate })
}

pub fn own_process_obj_exists() -> bool {
    ca::Process::with_pid(std::process::id() as i32).is_ok()
}
