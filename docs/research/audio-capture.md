# Audio capture for real-time transcription (Lowk)

Date: 2026-10-19. Scope: two streams, "me" (microphone) and "them" (single app or whole system), macOS 14+ (Apple Silicon) and Windows 10/11, Tauri 2 + Rust core.

Legend: **[V]** verified in a primary source or source code I read this session; **[I]** inference or recollection that must be confirmed by a spike before relying on it.

---

## 0. TL;DR / decisions

| Question | Recommendation |
|---|---|
| macOS "them" | **Core Audio process taps** (`CATapDescription` + private aggregate device) for both per-app and whole-system. ScreenCaptureKit only as fallback for macOS 14.0/14.1 (taps arrive in 14.2) and as a plan B if taps prove flaky. |
| macOS "me" | Plain input device via CoreAudio/cpal (`AVAudioEngine` input). Voice-processing (AEC) mode is optional, see section 3. |
| Windows "them" | **Process loopback** (`ActivateAudioInterfaceAsync`, `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`) with `INCLUDE_TARGET_PROCESS_TREE` for a chosen app; classic endpoint loopback (`AUDCLNT_STREAMFLAGS_LOOPBACK`) for whole system; our own app excluded via `EXCLUDE_TARGET_PROCESS_TREE` on our PID when in "whole system" mode. |
| Windows "me" | WASAPI shared-mode capture on the default/selected capture endpoint. |
| Echo | Do **not** rely on dedupe alone. Ship **WebRTC AEC3 (`webrtc-audio-processing` crate) fed by the "them" stream as the reference signal**, enabled by default only when the output device is not headphones, plus a cheap transcript-level dedupe as a safety net. |
| Crates | `cidre` (macOS CoreAudio taps, MIT) + `wasapi` 0.26 (Windows, MIT) + `cpal` 0.18 (mic only) + `rubato` 5 (resampling) + `webrtc-audio-processing` 2.1 (AEC) + `sysinfo` (process names). Writing a thin in-house `lowk-audio` crate with two platform backends behind one trait is better than adopting cpal for system capture. |
| Prior art to read (not copy blindly) | Meetily (MIT) and Char/Anarlog (MIT; `enterprise/` is commercial, do not copy from it). Both only capture **whole-system / default-endpoint**, not per-app. |

AGPL compatibility: every dependency recommended here is MIT, Apache-2.0, BSD-2/3, which are all one-way compatible into an AGPL-3.0 combined work. Keep their license notices in the distribution.

---

## 1. macOS

### 1.1 Options

**A. Core Audio process taps (recommended).** Introduced in macOS 14.2 per Recall.ai and the Rust `coreaudio`/cpal docs (AudioCap's README says 14.4; the SDK header marks the class `API_AVAILABLE(macos(12.0))` which is only a header artifact. Treat **14.4 as the safe floor** for shipping and gate the feature at runtime). **[V]** `CATapDescription.h` in the local SDK (Xcode on this machine) confirms:
- Constructors: `initStereoMixdownOfProcesses:` / `initMonoMixdownOfProcesses:` (include list), `initStereoGlobalTapButExcludeProcesses:` / `initMonoGlobalTapButExcludeProcesses:` (global minus list), plus device-specific variants (`initWithProcesses:andDeviceUID:withStream:`, `initExcludingProcesses:...`).
- `muteBehavior`: `CATapUnmuted` (default), `CATapMuted`, `CATapMutedWhenTapped`.
- `processes` (array of **AudioObjectIDs of process objects**, not PIDs), `UUID`, `name`, `isPrivate`.
- `bundleIDs` property exists only from macOS 26.0 (so on 14/15 you must go PID -> process object).
- Process object translation: `kAudioHardwarePropertyTranslatePIDToProcessObject` ('id2p'); list: `kAudioHardwarePropertyProcessObjectList` ('prs#'); per-process `kAudioProcessPropertyPID`, `...BundleID`, `...IsRunning`, `...IsRunningInput`, `...IsRunningOutput`.

Setup sequence **[V]** from AudioCap (BSD-2-Clause sample, https://github.com/insidegui/AudioCap): PID -> process object ID -> `CATapDescription` -> `AudioHardwareCreateProcessTap` -> read `kAudioTapPropertyFormat` -> build a private aggregate device whose tap list holds the tap UUID -> `AudioHardwareCreateAggregateDevice` -> IOProc via `AudioDeviceCreateIOProcIDWithBlock` -> `AudioDeviceStart`. Teardown must destroy IOProc, aggregate device and tap in order.

Permission UX **[V]**:
- Needs `NSAudioCaptureUsageDescription` in Info.plist (Xcode does not offer it in the dropdown; one report says `INFOPLIST_KEY_` build settings silently dropped it, so check the built plist; in Tauri add it via `Info.plist` merge in `tauri.conf.json` bundle.macOS).
- Prompt appears the first time recording starts ("would like to record audio from other apps"-style, category **System Audio Recording** in Privacy & Security). It is a one-click prompt that, per RecordKit, also reassures the user that the app cannot capture the screen. If denied or missing key: **tap returns silence (zeros), no error** (Meetily comment in `capture/core_audio.rs`, RecordKit guide).
- There is **no public API to check/request** this permission (AudioCap README). AudioCap uses private TCC SPI to preflight; that's fragile and un-notarizable-risky. Approach for Lowk: do a short probe tap at onboarding and detect "all zeros while something plays" -> show a deep link to System Settings. Caveat: zeros are also legitimate silence, so run the probe while playing a short test tone from our app on the *tapped* path (see 1.4) or pick a process that is currently `IsRunningOutput`.
- On macOS 26 the Screen Recording permission also grants system-audio access via Core Audio, but the usage-description key should still be present (RecordKit).
- Terminals/dev builds: when running unbundled (cargo run from iTerm/Terminal) the permission is attributed to the terminal and some terminals never prompt (AudioTee README). Test with a bundled `.app` from day one.

Sample format **[V]**: tap format is read from `kAudioTapPropertyFormat`; in practice float32 non-interleaved/interleaved at the output device rate (44.1/48/96 kHz) and stereo or mono depending on the constructor (AudioTee: default keeps device rate, 32-bit float; Meetily and Char both use `with_mono_global_tap_excluding_processes` and read `asbd().sample_rate`). Always read the ASBD and handle the rate changing mid-session (Char re-probes sample rate; Meetily tracks `current_sample_rate`).

Latency: IOProc cadence is the aggregate device buffer (typically 256-1024 frames, 5-20 ms). Delivery latency is not a problem; AudioTee defaults to 0.2 s chunks only because it is a stdout tool. **[I]** The tap is post-mixer of the process, pre-output-device, so the tapped signal leads the audible sound by the output device latency (important for AEC alignment, see 3).

Known problems **[V]**:
- Apple Developer Forums thread 825780 (macOS 26.5 beta): `AudioHardwareCreateProcessTap` delivered **all-zero buffers for minutes while audio was audible**, correlated (unconfirmed) with sample-rate renegotiation 44.1<->48 kHz and Bluetooth sleep/wake. Only workaround: full teardown and rebuild (stop IOProc, destroy aggregate, destroy tap, recreate all). No Apple answer. => Build a **watchdog**: if the process reports `IsRunningOutput` (or system output is active) and we see N seconds of exact zeros, rebuild the tap.
- Targeting a PID that is not currently producing audio can fail/exit (AudioTee README: "may fail"); in practice the process object does not exist until the process has touched Core Audio. => Retry/poll, or tap on demand when it appears in the process list.
- Aggregate device is bound to the default output device UID (Meetily/Char pass the output UID as main sub-device). When the user switches output (AirPods plug in), the aggregate must be rebuilt; listen to `kAudioHardwarePropertyDefaultOutputDevice`.
- Do not include both the output sub-device and the tap in the aggregate: Meetily documents that this produced duplicated audio ("echo") and they fixed it by keeping only the tap.

**B. ScreenCaptureKit audio.** Available macOS 12.3+ (audio: 13.0 baseline per `screencapturekit` crate; mic via `captureMicrophone` 15.0+). **[V]** from crate README + RecordKit:
- Permission: **Screen Recording** (more alarming prompt, and since macOS 15 periodic re-confirmation "continue to allow" dialogs appear **[I]**). App restart needed after granting.
- `SCStreamConfiguration`: `capturesAudio`, `excludesCurrentProcessAudio` (excludes our own sounds), `sampleRate` (8/16/24/48 kHz offered), `channelCount` 1 or 2. Native 16 kHz mono is attractive but quality of Apple's resampler is unknown; I'd still resample ourselves from 48 kHz **[I]**.
- Needs a content filter (display or window); per-app filtering is via `SCContentFilter(display:including:[SCRunningApplication])`, which works on **app level (bundle)** and thus handles multi-process apps like Chrome more naturally than PID taps. Audio of the included apps only. **[I]** (documented behavior; not re-verified this session).
- Downsides: Screen Recording permission, video pipeline overhead (must configure a tiny frame / low fps even for audio-only; macOS 15+/26 may allow audio-only streams **[I]**), the orange/purple recording indicator may show, and it conflicts with the stealth goal (a screen-capture entitlement/indicator is visible to the user and in Control Center).
- Keep as fallback for macOS 14.0-14.1 users and as plan B.

**C. Virtual audio drivers (BlackHole, Loopback).** Require installing a kernel/HAL driver and user rerouting. Rejected: UX and "lightweight" requirement.

**D. AudioTee** (MIT, https://github.com/makeusabrew/audiotee): a CLI built on taps that streams PCM to stdout, with `--include-processes`, `--exclude-processes`, `--mute`, `--sample-rate 16000` (converts to s16), `--chunk-duration`. Useful as a reference or as a sidecar, but in-process Rust via `cidre` avoids a second binary and a second TCC identity.

### 1.2 Per-app capture on macOS

Taps work on process objects, and "Zoom" or "Chrome" is a **group of processes**. Recall.ai: developers do not target "Chrome" or "Zoom" by name; browser audio may come from renderer/helper processes. Strategy:
1. Enumerate process objects from `kAudioHardwarePropertyProcessObjectList`.
2. Group by app: resolve each PID to its owning `.app` bundle (walk parent PIDs with `sysinfo`, or `NSRunningApplication` / `proc_pidpath` -> bundle path). Chrome helpers are `Google Chrome Helper (Renderer)` etc. inside the main bundle; group by bundle path/ID.
3. Build the tap with **all process objects of the group** in `processes` (include list, `exclusive=false`). Re-evaluate when processes appear/disappear (listen to `kAudioHardwarePropertyProcessObjectList` changes) and rebuild/extend the tap. **[I]** Whether `processes` can be updated on a live tap without rebuild is undocumented; assume rebuild (cheap, ~tens of ms).
4. Alternative for "everything except noise": global tap excluding processes (ourselves + user-chosen exclusions). This is the "whole system" mode and also the simplest fallback for browsers.

Browser tab for Meet: macOS offers no per-tab audio. Granularity is the browser app (all tabs). Document this in the UI. Real per-tab capture would need a browser extension (`chrome.tabCapture`), out of scope for now.

Mute behavior: keep `CATapUnmuted` (we must not silence the call for the user).

### 1.3 Microphone ("me") on macOS

- Standard input capture (cpal CoreAudio host or `cidre`/AVAudioEngine). Permission: **Microphone** TCC prompt + `NSMicrophoneUsageDescription` (and `com.apple.security.device.audio-input` entitlement under hardened runtime). Distinct from the system-audio prompt, so the user sees two prompts at onboarding (Microphone, System Audio Recording) and nothing else on the taps path. With SCK it would be three (Screen Recording added).
- Voice-processing I/O (AEC + noise suppression + AGC built into the OS) available via `AVAudioInputNode.setVoiceProcessingEnabled(true)`; `voiceProcessingOtherAudioDuckingConfiguration` (macOS 14.0+) controls ducking level **[V]** Apple docs listing; per an Apple engineer on the forums it **ducks other audio** (the call audio the user is listening to) when enabled, and users complain playback becomes quieter. That is a bad side effect for a stealth meeting assistant => see section 3.

### 1.4 Avoiding capture of our own app's sounds

- Whole-system tap: pass **our own process object** in the exclude list of `initMonoGlobalTapButExcludeProcesses` (both Meetily and Char pass an empty list, i.e. they do capture themselves; Lowk plays notification sounds / TTS, so exclude). Our own audio PID may need to be looked up after we first play something; better: we don't play audio on the system path at all, or create the process object lazily.
- Per-app mode: include list only contains the target app, so we're excluded by construction.
- SCK equivalent: `excludesCurrentProcessAudio = true`.
- Tauri webview audio plays from the WKWebView helper (`com.apple.WebKit.GPU`/WebContent) processes which are separate processes: **[I]** the web content audio is rendered through a WebKit XPC process, not our main PID, so excluding only `getpid()` may not be enough. Mitigation: never play audio from the webview during a call; if needed, enumerate our own children (`ppid` walk) and exclude all of them.

### 1.5 macOS: what the tap gives you not

Mic is separate (taps never include mic). Taps do not capture DRM-protected streams **[I]**. Bluetooth headsets in HFP mode drop to 16 kHz mono and trigger rate renegotiation, a known source of the "zeros" problem above.

---

## 2. Windows

### 2.1 Options

**Endpoint loopback (whole system).** `IAudioClient::Initialize` on a **render** endpoint with `AUDCLNT_STREAMFLAGS_LOOPBACK`, shared mode only **[V]** (Microsoft Learn, Loopback Recording). Event-driven loopback is supported from Windows 10 1703 (older versions needed a dummy render stream workaround). Captures the mix of everything that endpoint plays, regardless of session. If the system has silence, no packets are delivered in loopback (event-driven loopback may produce **no events during silence**), so the reader must tolerate gaps and insert zeros by wall clock/QPC position **[I]**. DRM-protected content is blocked by trusted drivers **[V]**.

**Process loopback (per-app).** `ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, ...)` with `AUDIOCLIENT_ACTIVATION_PARAMS { ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, ProcessLoopbackParams = { TargetProcessId, ProcessLoopbackMode } }`, where mode is `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE` or `PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE` **[V]**:
- `INCLUDE_...TREE`: only audio rendered by the target PID **and its child processes**.
- `EXCLUDE_...TREE`: everything except that process tree. This is the clean way to implement "whole system minus Lowk".
- Not tied to any single endpoint (follows the audio regardless of which device the app renders to), so no per-device clients needed **[V]** sample README.
- Target process with no active render streams -> you get **silence**, not an error **[V]**.
- **OS requirement**: Microsoft documents "Windows 10 Build 20348" as minimum **[V]**, whereas folklore says it works from 2004 (19041). Treat **19041+ with a runtime probe** as plausible but verify on a clean 2004/21H2 VM **[I]**; Windows 10 is end-of-support so most users are 22H2 (19045) or Windows 11 anyway. The API is called "ActivateAudioInterfaceAsync" on `VAD\Process_Loopback`; you cannot pre-query formats: the client is created without a mix format, so you must request a format yourself in `Initialize` (with `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | SRC_DEFAULT_QUALITY` to let the OS convert). The `wasapi` crate's example requests 32-bit float, 48 kHz, stereo with `autoconvert: true` **[V]**; ask for 16 kHz mono float directly to shift resampling to the OS? **[I]** (works with autoconvert in my understanding, quality of SRC_DEFAULT_QUALITY is fine for ASR but test). Format changes do not occur since it is a virtual device.
- Permission UX: **none**. No prompt for loopback or process loopback. Microphone access has the Windows privacy toggle (Settings > Privacy > Microphone, "Let desktop apps access your microphone"); a denied toggle yields silence/ACCESS_DENIED, so detect and deep-link `ms-settings:privacy-microphone`.

**Browsers / multi-process apps.** Chrome/Edge have many processes. Chromium's audio service runs in a separate **utility process** on Windows (sandboxed), launched by the browser process **[V]** Chromium services/audio README; hence audio is rendered by a **child of the main browser process**, not by renderers. Therefore `INCLUDE_TARGET_PROCESS_TREE` with the **main browser PID (the root of the tree)** captures all tabs' audio **[I]** (consistent with the API semantics and with how OBS "Application Audio Capture" is used with browsers, but I did not read OBS source this session; confirm with a spike). Pitfalls: (a) you must pick the **root** process (the one whose parent is not the same exe, or the one owning the top-level window), not an arbitrary renderer; (b) Edge WebView2 apps (Teams new client, etc.) have a `msedgewebview2.exe` tree whose root is the host app, so root-of-tree = the host exe; (c) Electron apps (Discord, Slack, old Teams) are the same pattern: audio service child of main; (d) Zoom, Teams, Discord: pick the root of the exe's tree. Per-tab separation is impossible on Windows via WASAPI (same as macOS).

**Alternatives.** Stereo Mix / virtual cables (rejected as on macOS). DirectShow/WinRT `AppCapture` (rejected). Windows 11 24H2+ "Windows Studio"/ system voice features: not needed.

### 2.2 Enumerating audio-producing apps (Windows)

Per **[V]** Microsoft docs: `IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eMultimedia)` -> `IMMDevice::Activate(IAudioSessionManager2)` -> `GetSessionEnumerator` -> for each `IAudioSessionControl` query `IAudioSessionControl2` -> `GetProcessId()`, `GetState()` (`AudioSessionStateActive` = currently rendering), `GetDisplayName`, `GetIconPath`, `IsSystemSoundsSession`. Also `IAudioSessionManager2::RegisterSessionNotification` for new sessions. Repeat for each active render endpoint, since an app can render to a non-default device (Char's Windows code polls for endpoint changes every 2 s and requires two confirmations before switching, to avoid bouncing on a one-off system sound **[V]**). Then map session PID to **tree root** (ToolHelp snapshot or `sysinfo`) and to exe name/icon (`QueryFullProcessImageNameW`, `SHGetFileInfo`/`ExtractIconEx`). The `wasapi` crate has a `processes` example listing devices and the processes using them with each session's peak level **[V]**.

Show "currently producing sound" using `IAudioMeterInformation::GetPeakValue` per session (non-zero peak = actually making noise, better than `Active` which is true when a muted call is open).

### 2.3 Windows mic ("me")

WASAPI shared capture on the default capture device (via `wasapi` crate or cpal). Option: open with `AudioCategory_Communications` (`IAudioClient2::SetClientProperties`) which can enable the **driver's/OS communications effects including AEC** on some devices; non-deterministic across hardware **[I]**. Do not depend on it.

---

## 3. Echo and duplication

Problem: with speakers (no headphones), "me" mic picks up the speaker playback of "them". Result: "them" speech is transcribed twice (once clean from the tap, once degraded via the mic), attributed to "me". Risk is worst for a product whose answers ("what did they just ask?") depend on correct speaker attribution.

Options:
1. **Transcript dedupe only.** Compare mic transcript segments to concurrent system transcript segments (fuzzy text match within a time window) and drop the mic copies. Cheap, no DSP. But: needs both streams transcribed (2x STT cost/CPU, bad for lightweight/local models), mic copy is lower quality and may differ, and real double-talk (user answers while they talk) gets mangled; also adds latency since you must wait for the system transcript before judging the mic one.
2. **OS-level voice processing on the mic.** macOS VoiceProcessingIO: effective but ducks other audio and is known to degrade playback volume **[V forum answer]**; it also can change the input device format and, when it works on the output reference, applies to the *default output device* only. Windows: communications category (device-dependent). Non-portable, unpredictable. Not recommended as primary.
3. **Software AEC with our own reference signal (recommended).** We already capture the exact far-end signal (the "them" tap/loopback). Feed it as the **render/reference** stream and the mic as the **capture** stream into WebRTC's AEC3 (`webrtc-audio-processing` crate, BSD-3-Clause, v2.1.0 published 2026-05-13, tracks upstream PulseAudio WebRTC AudioProcessing 2.x; builds via meson+ninja on macOS **[V]**; Windows build support unclear in README **[I]** — check; fallback: `aec-rs` which wraps Speex AEC, but it is unmaintained since Dec 2024). Practicalities:
   - APM works in 10 ms frames at 16/32/48 kHz. Do the alignment at 48 kHz or 16 kHz (our ASR rate) after resampling both streams to the same rate **before** AEC; running AEC at 16 kHz is fine and cheapest.
   - Reference must be time-aligned: the render signal reaches the mic after output-device latency + acoustic path. AEC3 estimates delay automatically up to a few hundred ms but you should supply `stream_delay_ms` as a hint: macOS from `kAudioDevicePropertyLatency + safety offset + buffer frames` of the output device; Windows from `IAudioClient::GetStreamLatency` of a render client or a measured calibration. Bluetooth output (150-300 ms) is the hard case; AEC3 handles ~up to 500 ms poorly **[I]**.
   - Use the *per-app* "them" stream as reference only if the user is not also hearing other apps; otherwise the reference is incomplete (music playing in another app leaks into mic uncancelled). Better: **when AEC is on, always tap the full system mix as the reference**, even if the transcription source is a single app. Cost: one extra tap/loopback (global mix) just for AEC; no extra STT. On macOS a second tap on the same aggregate is cheap; on Windows it's a second loopback client.
   - Char/Anarlog ships its own ONNX-based AEC crate (`crates/aec`, models 128/256/512 in repo, `realfft`, `anlg-onnx`), indicating that a neural AEC is another path; its quality/licensing of the models was not verified **[I]** (repo is MIT but model provenance unknown). Meetily does not do AEC at all, only "ducking and clipping prevention" and `nnnoiseless` (RNNoise port, BSD-3, v0.5.2) for denoise.
4. **Detect headphones and skip.** Check output device transport type: macOS `kAudioDevicePropertyTransportType` + data source (built-in speakers vs headphones via `kAudioDevicePropertyDataSource` 'ispk'/'hdpn'), Bluetooth/USB heuristics; Windows `PKEY_AudioEndpoint_FormFactor` (Speakers vs Headphones/Headset) from `IPropertyStore`. If headphones: AEC off (bypass, zero cost). If speakers: AEC on. Offer a manual toggle.

**Recommendation:** (3)+(4) as the default pipeline, plus a lightweight (1)-style safety net later: because "me" and "them" are labeled by stream, a simple energy/VAD gate also helps: when "them" VAD is active and AEC residual on mic is below a threshold, suppress mic VAD (avoids transcribing residual echo as "me"); keep the user's real double-talk by only suppressing when the mic energy is explained by the reference (coherence). Evaluate on recorded sessions with speakers at several volumes, then decide whether neural AEC (Char approach) is needed. Budget a spike of ~2 days: record mic+loopback on a MacBook (built-in speakers) and a Windows laptop, run AEC3 offline, measure WER of the mic channel with and without.

Fallback if AEC is unavailable/unstable on Windows build: option 1 only for that platform in alpha, with a UI hint "use headphones for best accuracy".

---

## 4. Rust crates and prior art

| Item | License | Notes |
|---|---|---|
| `cpal` 0.18.2 (Apache-2.0, pushed daily) | OK | Mic capture on both OSes works. README minimum-versions table says CoreAudio needs macOS 14.2 and "**loopback recording requires 14.6+**" **[V]**, implying cpal now exposes loopback via taps on macOS (global/device level, per-app unlikely). Windows WASAPI loopback via cpal exists, but cpal has **no process loopback** that I could confirm. Meetily pins cpal 0.15.3 / a git rev, Char has it as a workspace dep. Use cpal for the microphone only, or skip it and use `wasapi`+`cidre` for symmetry. |
| `cidre` 0.29 (MIT) | OK | Safe-ish Rust bindings to Apple frameworks; has `core_audio::TapDesc::with_mono_global_tap_excluding_processes`, `create_process_tap`, aggregate device keys (`tap_list`, `is_private`, `tap_auto_start`, `main_sub_device`), `StartedDevice`. Used by **both** Meetily and Char for taps **[V]** (source read). Process-include constructors: verify they are bound (the header ones exist; if not, drop to `objc2` for `CATapDescription` init). Pre-1.0, git-pinned in Meetily: pin the version. |
| `coreaudio-rs` 0.14.2 (MIT/Apache) | OK | Low-level AudioUnit; not needed. |
| `screencapturekit` 11.0.0 (doom-fish, Apache-2.0 OR MIT, active) | OK | macOS 13+ baseline; system audio needs only Screen Recording; mic needs macOS 15+ and `macos_15_0` feature, `NSMicrophoneUsageDescription` **[V]**. Fallback path only. |
| `wasapi` 0.26.0 (HEnquist, MIT, pushed 2026-10-13) | OK | Safe wrapper over the `windows` crate. **[V]** `AudioClient::new_application_loopback_client(process_id, include_tree)` (example `record_application`), `initialize_mta`, `StreamMode::EventsShared { autoconvert: true, ... }`, `get_next_packet_size`, `read_from_device_to_deque`, `wait_for_event`. Also device notifications (default device changed, added/removed) and a session/process listing example. MSRV 1.85 (example needs 1.88 because of `sysinfo`). Char uses it for the Windows loopback. Best fit. |
| `windows` 0.62.x (MIT/Apache) | OK | Needed for session enumeration (`IAudioSessionManager2`), property store (form factor), icons. Char's `audio-device` crate uses features `Win32_Media_Audio`, `Win32_Media_Audio_Endpoints`, `Win32_System_Com`, `Win32_UI_Shell_PropertiesSystem`, `Win32_Devices_FunctionDiscovery`. |
| `rubato` 5.0.1 (MIT) | OK | Resampler (Meetily used 0.15, Char 1.0). See section 6. |
| `webrtc-audio-processing` 2.1.0 (tonarino, BSD-3-Clause) | OK | AEC3, NS, AGC. Needs meson/ninja/clang to build bundled C++ on macOS **[V]**; Windows MSVC support to verify. |
| `nnnoiseless` 0.5.2 (BSD-3) | OK | RNNoise port; optional mic denoise. Beware: denoise can hurt ASR accuracy; test. |
| `sysinfo` (MIT) | OK | Process names/parents for grouping. |
| `ringbuf` | MIT/Apache | SPSC lock-free ring used by both Meetily and Char between RT callback and async consumer; copy that pattern. |
| AudioCap (insidegui, BSD-2) | OK | Reference for the tap sequence + permission preflight. Swift sample, not a crate. |
| AudioTee (makeusabrew, MIT) | OK | Reference/sidecar. |

### Existing products

- **Meetily** (Zackriya-Solutions/meetily, **MIT** per README and GitHub API; a separate paid "Pro" tier exists with extra features; Tauri + Next.js + Rust; last push 2026-09-15). macOS: `audio/capture/core_audio.rs` uses `cidre` global **mono** tap + private aggregate device + ringbuf; Windows: cpal WASAPI loopback (device-level, no process loopback); mixes mic+system with ducking/clipping prevention, `nnnoiseless`, `rubato`, ffmpeg sidecar. No AEC. Documents the "tap + output sub-device duplicates audio" pitfall. Borrowed code from whisper.cpp, Screenpipe, transcribe-rs.
- **Char / Hyprnote** (now `fastrepl/anarlog`, **MIT** with `LICENSE.enterprise` commercial for `enterprise/` only, Tauri 2). `crates/audio-actual`: macOS `speaker/macos.rs` is the same `cidre` global mono tap pattern with sample-rate re-probing and async ring; Windows `speaker/windows.rs` uses `wasapi` with polling of the default endpoint every 2 s (needs 2 confirmations to switch) and takes the first channel; `crates/aec` (ONNX neural AEC); `crates/resampler` on `rubato`; `crates/detect` (meeting-app detection via `sysinfo`, plus Zoom/mic-in-use detection); `crates/audio-device` enumerates devices (cidre / windows crates); `crates/tcc`. Neither project does per-app capture. Treat as the closest reference architecture; copy patterns, not the whole thing, and respect MIT notices if code is reused.
- **Cluely-style clones**: not found in this pass beyond the above; I did not locate a credible open-source Cluely clone with audio code worth reviewing. **[gap]**
- **Screenpipe** (mediar-ai/screenpipe, repo moved; license not confirmed this session: it was **not** verified, historically MIT with commercial parts **[I]**) is cited by Meetily as a code source; check before reusing anything.
- **Recall.ai blog** (https://www.recall.ai/blog/core-audio-taps) lists the real-world edge cases for taps: mute detection, mic/speaker changes, sync/drift, duplicate audio across streams, hard-to-target browsers.

---

## 5. Enumerating apps that currently produce audio (picker UI)

### macOS
1. `AudioObjectGetPropertyData(kAudioObjectSystemObject, kAudioHardwarePropertyProcessObjectList)` -> array of process `AudioObjectID`s **[V]** header.
2. For each: `kAudioProcessPropertyPID`, `kAudioProcessPropertyBundleID`, `kAudioProcessPropertyIsRunningOutput` (macOS 14+) **[V]** header constants.
   - Caveat **[V]** (forums): `IsRunningOutput` means the process has an *active output IO*, not that it is producing non-zero samples; a muted Zoom call still reports true. Listener for it had reports of not firing (thread 770348: only `IsRunning` and `Devices` notified in macOS 15.0.1), so **poll** at 1-2 Hz while the picker is open and subscribe to the object list change (`prs#`) to catch new processes.
3. Resolve PID -> app: `NSRunningApplication(processIdentifier:)` for name/icon of GUI apps; for helpers (Chrome Helper) walk up to the parent `.app` bundle and group; show one row per app with the helper's output state OR-ed. Filter out our own app tree and system daemons (`coreaudiod`, `com.apple.*` unless useful: "System sounds").
4. A level meter per candidate would need a tap per candidate; skip. Optionally show a transient "audio" indicator using `IsRunningOutput` only.

### Windows
As in 2.2: `IAudioSessionManager2` per active render endpoint, `IAudioSessionControl2::GetProcessId` + `GetState` + `IAudioMeterInformation::GetPeakValue` for a live level; group PIDs by exe tree root; icon via shell APIs; refresh by `RegisterSessionNotification` + 500 ms poll of the meter. Include the special row "Whole system".

### Both: default selection rules
Remember last choice by bundle ID/exe name (not PID). Heuristic auto-suggest: known meeting apps (Zoom, Teams, Discord, Slack Huddle, Chrome/Edge/Safari/Firefox) with output active. The Char `detect` crate shows a precedent for detecting running meeting apps.

---

## 6. Resampling, buffering, latency

Target for ASR: **16 kHz, mono, f32 (or i16)**, 20-100 ms frames to the STT front end. VAD (Silero-like) typically wants 30-32 ms frames at 16 kHz **[I]**.

Pipeline per stream (identical for me/them):

```
RT audio callback (CoreAudio IOProc / WASAPI event thread)
  -> convert to f32, downmix to mono (average; tap can already be mono)
  -> push to lock-free SPSC ring (no allocation, no locks, drop-oldest + counter on overflow)
processing thread
  -> rubato resampler (device rate -> 16 kHz), fixed-size chunks
  -> [optional AEC on the "me" stream with the "them" reference, 10 ms frames at 16 kHz]
  -> frame to STT/VAD (timestamped with a monotonic clock)
```

- **Timestamps:** stamp every chunk at capture time with a shared monotonic clock (macOS `mach_absolute_time` from the IOProc `AudioTimeStamp`, Windows QPC from `GetBuffer`'s `pu64QPCPosition`) so "me" and "them" can be interleaved in the transcript and AEC can compute delay. Mic and loopback devices drift (separate clocks): AEC3 tolerates small drift; for the mixer, resample-by-drift correction is not needed for transcription since streams are transcribed separately.
- **Resampler:** `rubato` (MIT/Apache): use the FFT-based `Fft`/`FftFixedIn` for fixed ratios 48000->16000 (3:1 exact) and 44100->16000 (147:320); for 96k/192k devices still fine. Use async sinc (`SincFixedIn`, low-ish quality) only if the rate can drift; since ASR is the consumer, `FftFixedIn` with 480-1024 frame chunks gives ~10-20 ms of added latency. API changed in rubato 1.x/5 (audioadapter-buffers; Char's resampler crate shows 1.0 usage). Re-create the resampler on sample-rate change events (see macOS tap rate re-negotiation). Alternative: ask the OS to deliver 16 kHz (Windows `autoconvert`, SCK `sampleRate`), but quality/behavior is unproven and CoreAudio taps can't be told a rate (they follow the device), so one resampler path is needed anyway.
- **Mono downmix:** Char's Windows backend takes only the first channel; averaging channels is safer for stereo calls where voices may be panned.
- **Buffers:** ring of ~2-4 s per stream so a stalled STT does not drop data, but keep the processing thread real-time-ish. Choose callback buffer 256-512 frames; processing hop 20 ms; end-to-end capture-to-STT-input latency budget **< 60 ms** (device buffer 10 ms + resample 20 ms + hop 20 ms), leaving the budget (< 1 s total per BRIEF) to the STT engine.
- **Backpressure/drops:** count dropped samples (Meetily tracks `consecutive_drops`; Char tracks `dropped_samples`); surface a debug metric.
- **Silence handling:** Windows loopback sends no packets during silence; macOS taps send zeros. Normalize both: emit zero-filled frames by clock so downstream VAD sees continuous time. Add the "all-zero but should be audible" watchdog for macOS (section 1.1).
- **Device changes:** listen for default input/output changes (macOS property listeners; Windows `IMMNotificationClient` — the `wasapi` crate exposes notifications), rebuild streams, and keep the logical stream continuous (timestamps keep running).
- **Threading:** one OS thread per stream for the COM/WASAPI event loop (MTA init required, `initialize_mta()`); IOProc is already RT. No tokio inside the RT callback.

---

## 7. Recommended architecture

A Rust crate `lowk-audio` with a platform-neutral trait and two backends, running its own threads, exposing an async `Stream<Item = Frame16k>` per channel to the rest of the Tauri core.

```
trait SystemSource { fn list_candidates() -> Vec<AudioApp>; fn start(target: Target) -> Receiver; }
enum Target { App{ id: AppId /*bundle id | exe path*/ }, WholeSystem }
struct Frame { t_mono_ns: u64, channel: Me|Them, pcm16k: [f32] }
```

### macOS backend
- `them`: `cidre` Core Audio tap.
  - `App`: resolve app -> set of PIDs -> process object IDs -> `initMonoMixdownOfProcesses` (verify exposure in cidre; else `objc2`), `CATapUnmuted`, private tap, aggregate device with only the tap (no output sub-device).
  - `WholeSystem`: `initMonoGlobalTapButExcludeProcesses([our process objects])`.
  - Supervisor: rebuilds on default output change, on sample-rate change, on zero-watchdog trip, on target process (re)appearance.
  - Gate by `macOS >= 14.4` at runtime; below, use SCK (`excludesCurrentProcessAudio = true`) or show "update macOS".
- `me`: input device (cpal or cidre). No voice-processing by default.
- Permissions: Info.plist keys `NSMicrophoneUsageDescription`, `NSAudioCaptureUsageDescription`; entitlement `com.apple.security.device.audio-input`. Onboarding: two prompts max, each with a pre-explanation screen; test-probe + "I don't hear anything?" recovery link to System Settings > Privacy & Security.
- AEC (speakers mode): tap a full-system mono mix as reference (second tap) -> `webrtc-audio-processing`.

### Windows backend
- `them`:
  - `App`: `wasapi::AudioClient::new_application_loopback_client(root_pid, include_tree=true)`, 16 kHz mono f32 with autoconvert (fallback: 48 kHz stereo + own resampler), event-driven. Root PID chosen as tree root for the exe. If the app restarts, re-resolve by exe path.
  - `WholeSystem`: `new_application_loopback_client(our_pid, include_tree=false)` meaning EXCLUDE (so Lowk is excluded from the capture), else classic loopback on the default render endpoint if process loopback is unavailable (OS < supported) and accept that our own sounds leak.
  - Gate: probe `ActivateAudioInterfaceAsync` result at startup; if unsupported, only "Whole system" via classic loopback.
- `me`: `wasapi` shared capture, event-driven, on the selected capture device.
- No permission prompts for `them`; microphone privacy toggle check for `me`.
- AEC reference = system-wide render loopback (classic loopback or exclude-self process loopback) rather than the app-only stream.

### Shared
- Same `Frame` format, timestamps from a monotonic source, SPSC rings, `rubato`, optional AEC stage, VAD, then STT. UI picker driven by `list_candidates()` (section 5) with 1 Hz refresh.
- Telemetry (local only): per-stream dropped samples, tap rebuild count, watchdog trips.

---

## 8. Open questions / spikes (in priority order)

1. **macOS floor and per-app multi-process:** confirm taps work with `processes = [all Chrome/Zoom/Teams helper process objects]` on macOS 14.4, 15.x, 26.x; test hot-adding a helper process. Confirm cidre exposes mixdown-of-processes (or use `objc2`).
2. **macOS zero-buffer watchdog:** reproduce and tune; test Bluetooth headsets (HFP rate switch) and sleep/wake.
3. **Windows process loopback on 19041/19045 vs 20348+:** microsoft docs say 20348; verify real behavior on Win10 22H2 and Win11 24H2/25H2; verify Chrome/Edge/Teams/Discord/Zoom root-PID capture and silence-on-no-stream behavior.
4. **AEC3 on Windows build** (`webrtc-audio-processing` with MSVC) and measured echo suppression with built-in laptop speakers; delay hints for Bluetooth.
5. **Own-process audio exclusion** in Tauri: which processes actually render webview audio (WKWebView helper on macOS, `msedgewebview2.exe` child on Windows — the latter *is* excluded by tree-exclude on our PID, good).
6. **Permission-check approach** without private TCC SPI.
7. **Windows loopback silence gaps** and drift against the mic clock.
8. Cluely-like open-source clones audio code (none found), and Screenpipe's license.

---

## Sources

- Core Audio taps: AudioCap README and code (BSD-2) https://github.com/insidegui/AudioCap ; AudioTee (MIT) https://github.com/makeusabrew/audiotee ; Recall.ai https://www.recall.ai/blog/core-audio-taps ; RecordKit guide https://nonstrict.eu/recordkit/guides/system-audio-recording.html ; Apple docs "Capturing system audio with Core Audio taps" https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps (page body did not load; details taken from local SDK headers `CATapDescription.h`, `AudioHardware.h` in Xcode's MacOSX.sdk) ; Apple Developer Forums zero-buffer thread https://developer.apple.com/forums/thread/825780 ; `kAudioProcessPropertyIsRunningOutput` thread https://developer.apple.com/forums/thread/770348 ; voice-processing ducking https://developer.apple.com/documentation/avfaudio/avaudioinputnode/voiceprocessingotheraudioduckingconfiguration and WWDC23 10235 notes https://wwdcnotes.com/notes/wwdc23/10235 ; Tahoe 26.1 permission-list bug https://developer.apple.com/forums/thread/807898
- ScreenCaptureKit: crate README https://github.com/doom-fish/screencapturekit-rs ; SCStreamConfiguration properties are from my recollection of Apple docs (the page body did not load) **[I]**.
- Windows: Loopback Recording https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording ; `AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS` https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params ; Application Loopback sample https://github.com/microsoft/windows-classic-samples/tree/main/Samples/ApplicationLoopback and https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/ ; `IAudioSessionManager2` https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiosessionmanager2 ; Chromium audio service https://chromium.googlesource.com/chromium/src/+/HEAD/services/audio/README.md
- Crates/projects: https://github.com/HEnquist/wasapi-rs (MIT; example `record_application.rs`) ; https://github.com/RustAudio/cpal (Apache-2.0) ; https://github.com/doom-fish/screencapturekit-rs ; https://github.com/yury/cidre (MIT) ; https://github.com/tonarino/webrtc-audio-processing (BSD-3) ; crates.io API for versions/licenses of rubato, nnnoiseless, wasapi, cpal, cidre, screencapturekit, aec-rs ; Meetily https://github.com/Zackriya-Solutions/meetily (files read: `frontend/src-tauri/src/audio/capture/core_audio.rs`, `devices/platform/windows.rs`, `Cargo.toml`) ; Char/Anarlog https://github.com/fastrepl/anarlog (renamed from fastrepl/hyprnote; files read: `crates/audio-actual/src/speaker/{macos,windows}.rs`, `crates/{aec,audio-device,resampler,detect}/Cargo.toml`, `crates/aec/src/lib.rs`).
