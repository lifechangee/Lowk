import Foundation
import AVFoundation
import Speech

// applestt probe
// applestt run <audio> --engine speech|dictation|sfs --locale ru-RU [--speed 1] [--chunk-ms 160] [--out file.jsonl]
// Emits JSONL: {"t": wall seconds since first audio chunk fed, "final": bool, "text": full snapshot (finalized + volatile)}
// and a last line {"summary": ...}.

func arg(_ name: String, _ def: String? = nil) -> String? {
    let a = CommandLine.arguments
    if let i = a.firstIndex(of: "--" + name), i + 1 < a.count { return a[i + 1] }
    return def
}
func now() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1e9 }
func err(_ s: String) { FileHandle.standardError.write((s + "\n").data(using: .utf8)!) }

func rssMB() -> Double {
    var info = mach_task_basic_info(); var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size / MemoryLayout<natural_t>.size)
    let kr = withUnsafeMutablePointer(to: &info) { $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), $0, &count) } }
    return kr == KERN_SUCCESS ? Double(info.resident_size) / 1048576 : 0
}
func cpuSeconds() -> Double {
    var ru = rusage(); getrusage(RUSAGE_SELF, &ru)
    return Double(ru.ru_utime.tv_sec + ru.ru_stime.tv_sec) + Double(ru.ru_utime.tv_usec + ru.ru_stime.tv_usec) / 1e6
}

func loadPCM(_ path: String, format: AVAudioFormat) throws -> AVAudioPCMBuffer {
    let f = try AVAudioFile(forReading: URL(fileURLWithPath: path))
    let inBuf = AVAudioPCMBuffer(pcmFormat: f.processingFormat, frameCapacity: AVAudioFrameCount(f.length))!
    try f.read(into: inBuf)
    guard let conv = AVAudioConverter(from: f.processingFormat, to: format) else { throw NSError(domain: "conv", code: 1) }
    let ratio = format.sampleRate / f.processingFormat.sampleRate
    let out = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(Double(inBuf.frameLength) * ratio) + 4096)!
    var given = false
    var e: NSError?
    conv.convert(to: out, error: &e) { _, st in
        if given { st.pointee = .endOfStream; return nil }
        given = true; st.pointee = .haveData; return inBuf
    }
    if let e { throw e }
    return out
}
func slice(_ b: AVAudioPCMBuffer, _ start: Int, _ n: Int) -> AVAudioPCMBuffer {
    let len = min(n, Int(b.frameLength) - start)
    let o = AVAudioPCMBuffer(pcmFormat: b.format, frameCapacity: AVAudioFrameCount(len))!
    o.frameLength = AVAudioFrameCount(len)
    let ch = Int(b.format.channelCount)
    if let s = b.floatChannelData, let d = o.floatChannelData { for c in 0..<ch { memcpy(d[c], s[c] + start, len * 4) } }
    else if let s = b.int16ChannelData, let d = o.int16ChannelData { for c in 0..<ch { memcpy(d[c], s[c] + start, len * 2) } }
    return o
}

final class Emitter: @unchecked Sendable {
    let lock = NSLock(); var t0 = now(); var lines: [String] = []; var finals = ""; var volatile = ""
    var nEvents = 0
    func emit(final: Bool, text: String) {
        lock.lock(); defer { lock.unlock() }
        let t = now() - t0
        let obj: [String: Any] = ["t": (t * 1000).rounded() / 1000, "final": final, "text": text]
        let d = try! JSONSerialization.data(withJSONObject: obj, options: [])
        lines.append(String(data: d, encoding: .utf8)!); nEvents += 1
    }
}

func probe() async {
    print("macOS \(ProcessInfo.processInfo.operatingSystemVersionString)")
    let st = await SpeechTranscriber.supportedLocales
    let dt = await DictationTranscriber.supportedLocales
    let sti = await SpeechTranscriber.installedLocales
    let dti = await DictationTranscriber.installedLocales
    print("SpeechTranscriber.isAvailable = \(SpeechTranscriber.isAvailable)")
    print("SpeechTranscriber supported (\(st.count)): " + st.map { $0.identifier }.sorted().joined(separator: ", "))
    print("SpeechTranscriber installed: " + sti.map { $0.identifier }.sorted().joined(separator: ", "))
    print("DictationTranscriber supported (\(dt.count)): " + dt.map { $0.identifier }.sorted().joined(separator: ", "))
    print("DictationTranscriber installed: " + dti.map { $0.identifier }.sorted().joined(separator: ", "))
    let sf = SFSpeechRecognizer.supportedLocales()
    print("SFSpeechRecognizer supported (\(sf.count)); ru: \(sf.filter { $0.identifier.hasPrefix("ru") }.map { $0.identifier })")
    for id in ["ru-RU", "en-US"] {
        let r = SFSpeechRecognizer(locale: Locale(identifier: id))
        print("SFSpeechRecognizer \(id): exists=\(r != nil) onDevice=\(r?.supportsOnDeviceRecognition ?? false) available=\(r?.isAvailable ?? false)")
    }
    print("SFSpeech auth status = \(SFSpeechRecognizer.authorizationStatus().rawValue)")
}

func runAnalyzer(path: String, engine: String, locale: Locale, speed: Double, chunkMs: Int, outPath: String?, download: Bool) async throws {
    let em = Emitter()
    var modules: [any SpeechModule] = []
    var resultsTask: Task<Void, Error>
    let transcriber: SpeechTranscriber?
    let dictation: DictationTranscriber?
    if engine == "speech" {
        let t = SpeechTranscriber(locale: locale, transcriptionOptions: [], reportingOptions: [.volatileResults], attributeOptions: [.audioTimeRange])
        transcriber = t; dictation = nil; modules = [t]
    } else {
        let d = DictationTranscriber(locale: locale, contentHints: [], transcriptionOptions: [], reportingOptions: [.volatileResults, .frequentFinalization], attributeOptions: [])
        dictation = d; transcriber = nil; modules = [d]
    }
    let st = await AssetInventory.status(forModules: modules)
    err("asset status: \(st)")
    if st != .installed {
        if download {
            if let req = try await AssetInventory.assetInstallationRequest(supporting: modules) {
                err("downloading assets..."); try await req.downloadAndInstall(); err("downloaded")
            } else { err("no installation request returned (assets probably already present); continuing") }
        } else { err("assets not installed; run with --download"); throw NSError(domain: "assets", code: 2) }
    }
    let analyzer = SpeechAnalyzer(modules: modules)
    guard let fmt = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: modules) else { throw NSError(domain: "fmt", code: 3) }
    err("analyzer format: \(fmt)")
    let buf = try loadPCM(path, format: fmt)
    let (stream, cont) = AsyncStream<AnalyzerInput>.makeStream()
    // results consumer
    resultsTask = Task {
        var finals = ""
        if let t = transcriber {
            for try await r in t.results {
                let s = String(r.text.characters)
                if r.isFinal { finals += (finals.isEmpty ? "" : " ") + s.trimmingCharacters(in: .whitespaces); em.emit(final: true, text: finals) }
                else { em.emit(final: false, text: (finals + " " + s).trimmingCharacters(in: .whitespaces)) }
            }
        } else if let d = dictation {
            for try await r in d.results {
                let s = String(r.text.characters)
                if r.isFinal { finals += (finals.isEmpty ? "" : " ") + s.trimmingCharacters(in: .whitespaces); em.emit(final: true, text: finals) }
                else { em.emit(final: false, text: (finals + " " + s).trimmingCharacters(in: .whitespaces)) }
            }
        }
    }
    try await analyzer.start(inputSequence: stream)
    let sr = fmt.sampleRate
    let n = Int(sr * Double(chunkMs) / 1000)
    let total = Int(buf.frameLength)
    let cpu0 = cpuSeconds(); var peak = rssMB()
    em.t0 = now(); let t0 = em.t0
    var i = 0
    while i < total {
        let len = min(n, total - i)
        let due = t0 + Double(i + len) / sr / speed
        let d = due - now(); if d > 0 { try await Task.sleep(nanoseconds: UInt64(d * 1e9)) }
        cont.yield(AnalyzerInput(buffer: slice(buf, i, len)))
        i += len
        peak = max(peak, rssMB())
    }
    let tAudioEnd = now()
    cont.finish()
    try await analyzer.finalizeAndFinishThroughEndOfInput()
    try await resultsTask.value
    let tDone = now()
    let dur = Double(total) / sr
    let summary: [String: Any] = ["summary": ["engine": engine, "locale": locale.identifier, "duration_s": dur, "final_after_eos_s": (tDone - tAudioEnd) * speed,
        "cpu_s": cpuSeconds() - cpu0, "peak_rss_mb": max(peak, rssMB()), "events": em.nEvents, "speed": speed]]
    em.lines.append(String(data: try JSONSerialization.data(withJSONObject: summary), encoding: .utf8)!)
    let out = em.lines.joined(separator: "\n") + "\n"
    if let p = outPath { try out.write(toFile: p, atomically: true, encoding: .utf8) } else { print(out) }
}

func runSFS(path: String, locale: Locale, speed: Double, chunkMs: Int, outPath: String?) async throws {
    let auth = await withCheckedContinuation { c in SFSpeechRecognizer.requestAuthorization { c.resume(returning: $0) } }
    err("SFS auth: \(auth.rawValue)")
    guard auth == .authorized else { throw NSError(domain: "auth", code: 4) }
    guard let rec = SFSpeechRecognizer(locale: locale), rec.supportsOnDeviceRecognition else { err("on-device not supported for \(locale.identifier)"); throw NSError(domain: "sfs", code: 5) }
    let fmt = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16000, channels: 1, interleaved: false)!
    let buf = try loadPCM(path, format: fmt)
    let em = Emitter()
    let req = SFSpeechAudioBufferRecognitionRequest()
    req.requiresOnDeviceRecognition = true; req.shouldReportPartialResults = true
    let done = DispatchSemaphore(value: 0)
    let task = rec.recognitionTask(with: req) { res, error in
        if let res { em.emit(final: res.isFinal, text: res.bestTranscription.formattedString); if res.isFinal { done.signal() } }
        else if let error { err("sfs error: \(error)"); done.signal() }
    }
    let n = 2560; let total = Int(buf.frameLength) // 160 ms @16k
    let cpu0 = cpuSeconds(); var peak = rssMB()
    em.t0 = now(); let t0 = em.t0
    var i = 0
    while i < total {
        let len = min(n, total - i)
        let due = t0 + Double(i + len) / 16000 / speed
        let d = due - now(); if d > 0 { try await Task.sleep(nanoseconds: UInt64(d * 1e9)) }
        req.append(slice(buf, i, len)); i += len; peak = max(peak, rssMB())
    }
    let tEnd = now(); req.endAudio()
    _ = await Task.detached { done.wait(timeout: .now() + 30) }.value
    let tDone = now(); _ = task
    let summary: [String: Any] = ["summary": ["engine": "sfs", "locale": locale.identifier, "duration_s": Double(total) / 16000, "final_after_eos_s": (tDone - tEnd) * speed,
        "cpu_s": cpuSeconds() - cpu0, "peak_rss_mb": max(peak, rssMB()), "events": em.nEvents, "speed": speed]]
    em.lines.append(String(data: try JSONSerialization.data(withJSONObject: summary), encoding: .utf8)!)
    let out = em.lines.joined(separator: "\n") + "\n"
    if let p = outPath { try out.write(toFile: p, atomically: true, encoding: .utf8) } else { print(out) }
}

let args = CommandLine.arguments
Task {
    do {
        if args.count > 1 && args[1] == "probe" { await probe() }
        else if args.count > 2 && args[1] == "run" {
            let engine = arg("engine", "speech")!
            let loc = Locale(identifier: arg("locale", "en-US")!)
            let speed = Double(arg("speed", "1")!)!
            let chunk = Int(arg("chunk-ms", "160")!)!
            if engine == "sfs" { try await runSFS(path: args[2], locale: loc, speed: speed, chunkMs: chunk, outPath: arg("out")) }
            else { try await runAnalyzer(path: args[2], engine: engine, locale: loc, speed: speed, chunkMs: chunk, outPath: arg("out"), download: args.contains("--download")) }
        } else { err("usage: applestt probe | run <audio> --engine speech|dictation|sfs --locale ru-RU [--download]") }
    } catch { err("ERROR: \(error)"); exit(1) }
    exit(0)
}
RunLoop.main.run()
