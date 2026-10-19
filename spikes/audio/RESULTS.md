# Audio capture spike - results (2026-10-19)

Machine: MacBook Air M4, macOS 27.0.1, Rust 1.99, cidre 0.29, rubato 0.16. Run from a shell spawned by the Claude desktop app (TCC "responsible process" = that app).

## How to run
```
export PATH=~/.cargo/bin:$PATH
cd spikes/audio && cargo build --release
B=target/release/lowk-audio-spike
$B list [--all]                                   # processes producing audio, helpers grouped under their .app
$B capture --app com.google.Chrome --duration 10 --out out/x   # or --app <pid|name>
$B capture --system --duration 10 --out out/x     # global tap, our own process excluded
  flags: --no-mic, --force-rebuild-after S (exercises watchdog rebuild path)
$B gen-tones tones && afplay tones/tone_440.wav & afplay tones/tone_880.wav &   # isolation test sources
$B analyze out/x/them.wav                         # Goertzel level at 440/880 Hz
```
Outputs per run: `me.wav` (mic), `them.wav` (tap), both 16 kHz mono s16, plus `*_latency.csv` (per output block: cb latency, newest-sample age, RMS dBFS, peak). stderr prints 1 line/s per stream and latency percentiles at exit. `build.rs` adds rpath `/usr/lib/swift` (cidre default features link Swift libs).

## Layout
- `src/mac.rs` Core Audio: process enumeration/grouping, process tap + private aggregate device, mic IOProc.
- `src/dsp.rs` platform-independent: mono chunks -> rubato `FftFixedIn` -> 16 kHz WAV, RMS, latency, zero-run detection.
- `src/win.rs` Windows (cfg(windows)), `src/main.rs` CLI.

## Measured (release build, 10 s, system tap + mic, 48 kHz native)
| stream | IOProc latency* p50 / p95 / max | newest sample age at 16 kHz output p50 / p95 / max |
|---|---|---|
| them (tap) | 0.83 / 0.90 / 1.03 ms | 0.90 / 1.01 / 1.17 ms |
| me (mic) | 1.02 / 1.20 / 1.57 ms | 1.08 / 1.30 / 2.23 ms |
*now (host time at callback) minus capture time of the buffer's last sample (`input_time.host_time` + frames/rate). Buffers are ~10.7 ms (512 frames @ 48k) on both streams. Debug builds: p95 ~3 ms, one 40 ms outlier. Note: ~2 ms "age" excludes the buffer's own duration (one 512-frame buffer = 10.7 ms is the real floor for the oldest sample) and the resampler's FFT delay (rubato `output_delay`, a few ms; not measured). Capture is not the latency bottleneck - STT will be.

## Isolation (two afplay processes, 440 Hz and 880 Hz, same output device)
| capture | 440 Hz | 880 Hz |
|---|---|---|
| `--app <pid of 440 player>` | 0.300 | 0.000 |
| `--app <pid of 880 player>` | 0.000 | 0.300 |
| `--system` (both playing) | 0.300 | 0.300 |
| `--system` (only 880 left) | 0.000 | 0.300 |
| `--app com.google.Chrome` (Chrome playing 880 wav tab, afplay 440 playing) | 0.000 | 0.300 |
Per-app taps are perfectly isolated (amplitudes are Goertzel RMS, 0.3 = the source level). Chrome's three audio-registered processes (main, GPU/network helpers) grouped under /Applications/Google Chrome.app and tapped as one mixdown. Mic bleed of the speakers into `me.wav` was about -33 dBFS for tone in `me` (laptop speakers at moderate volume) - echo exists, see AEC.

## Silent-tap watchdog
Implemented in `main.rs`: worker counts consecutive all-zero input ms (`Shared.zero_ms`); if >= 2 s of exact zeros AND a target process reports `IsRunningOutput` AND >= 5 s since last rebuild, tap is fully torn down (stop IOProc, destroy aggregate, destroy tap) and rebuilt, logged as `[watchdog] REBUILD TAP`. Also rebuilds on target process-set change (helper spawned). It fired for real once: an `afplay` pid that was alive but producing zeros (see What broke) -> 4.8 s of zeros detected, rebuilt. I could not reproduce the Apple-forum 44.1<->48 kHz zero bug on demand, so the "rebuild fixes it" claim is untested; the rebuild path itself was exercised with `--force-rebuild-after 2`: capture resumed with no silent blocks, gap in the output about 55 ms of lost audio (teardown + recreate ~75 ms wall).

## What broke / surprises
1. One early per-app run returned only zeros for a live-looking `afplay` pid (pgrep alive) while another run on a fresh afplay worked. Cause not established (afplay finishing / previous prompt blocking ~10 s start-up of that first run). Takeaway: zeros + `IsRunningOutput` can legitimately happen; a watchdog rebuild is harmless but do not alarm the user on first occurrence.
2. First run took ~10 s before the mic started (TCC mic prompt/first use). Afterwards no delays. No permission prompt blocked the spike; both mic and system-audio tap worked from this host app. Which binary needs which permission: the *responsible process* (here the Claude desktop app launching the shell, or Terminal/iTerm when run there) needs Microphone and System Audio Recording. A shipped Lowk.app needs `NSMicrophoneUsageDescription` + `NSAudioCaptureUsageDescription` in Info.plist; **not tested from a bundled .app here**.
3. A target pid must already have a Core Audio process object (it appears only once the process touched audio): `capture --app <idle pid>` fails with a clear error; real app needs retry/poll.
4. Own-process exclusion in `--system` requires our process object to exist; it only does after we have an IOProc running, so the mic is started first. Log shows `own excluded: true`. I did not play audio from the spike itself to prove the exclusion works; verify in the real app.
5. cidre: `default-features = false` does not compile (needs dispatch/blocks); default features link Swift, hence the rpath. cidre `TapDesc` constructors work as-is; pids -> `ca::Process::with_pid`.
6. Process grouping by outermost `.app` path works for Chrome/Claude/Telegram. Not solved: WebKit audio lives in `com.apple.WebKit.GPU` (system XPC service, no .app), so Safari audio is a separate group - needs a parent/responsibility mapping (e.g. `responsibility_get_pid_responsible_for_pid`) in the real app.
7. Chrome registers audio on the browser app level, as the research doc said; no per-tab separation.

## Windows (`cfg(windows)`, `src/win.rs`)
Type-checks: `cargo check --target x86_64-pc-windows-msvc` passes with `wasapi` 0.26 + `sysinfo`. Not run (no Windows host here), so unverified at runtime. Uses `AudioClient::new_application_loopback_client(pid, include_tree)`: app mode = tree root (walk parents while same exe name) with include_tree=true; system mode = our own PID with include_tree=false (exclude tree). Listing via `IAudioSessionManager2` sessions + peak meter (peak > 0 rather than Active). Gaps: capture-time stamps are approximated (QPC mapping TODO) so Windows latency numbers would read ~0; silent-tap watchdog not wired; mic uses autoconvert to 48k stereo float.

## AEC (optional)
`webrtc-audio-processing = { version = "2.1", features = ["bundled"] }` builds and links on macOS in ~56 s clean (scratch crate, not part of this spike). Needed `meson` + `ninja`, installed with `uv tool install meson ninja` (no Homebrew). Only build + a trivial run were checked; no echo-cancellation quality test. Windows build not checked.

## Recommendations for the real app
- Use cidre process taps for macOS as in `mac.rs`: mono mixdown for app mode, global-minus-self for system. Read the ASBD rate at start and expect it to change; `dsp.rs` already rebuilds the resampler.
- Resample in a worker thread (IOProc only copies + sends; the spike allocates a Vec per callback - use a lock-free ring buffer in production).
- Keep the group-by-.app approach; re-enumerate on a timer or listen to `kAudioHardwarePropertyProcessObjectList` changes and rebuild (rebuild costs ~55 ms audio gap).
- Always start mic first, then tap, so own-process exclusion works. Handle default output change by rebuilding (aggregate is tied to the output device UID) - not exercised here.
- Test the permission flow from a signed/bundled .app early; add a probe tap with a known tone at onboarding.
- Keep watchdog thresholds: 2 s zeros + IsRunningOutput, 5 s rebuild cooldown, surface to UI only after repeated failures.
- Latency budget is dominated by STT: capture + resample is ~10-15 ms. Use 20-30 ms chunks into STT; not an issue.
