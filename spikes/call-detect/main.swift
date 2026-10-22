// call-detect spike: lists processes using mic/output via CoreAudio process objects
// and prints transitions. Polls (default 1 s); property listeners on the process list /
// IsRunningInput work too (see docs/roadmap/call-detect.md).
// Reads NO audio and NO window contents. Usage: call-detect [--once] [--interval 1.0]
// Build: swiftc -O main.swift -o call-detect
import CoreAudio
import Foundation

func prop<T>(_ obj: AudioObjectID, _ sel: AudioObjectPropertySelector, _ def: T) -> T {
    var a = AudioObjectPropertyAddress(mSelector: sel, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    var v = def
    var sz = UInt32(MemoryLayout<T>.size)
    return AudioObjectGetPropertyData(obj, &a, 0, nil, &sz, &v) == noErr ? v : def
}

func bundleID(_ obj: AudioObjectID) -> String {
    var a = AudioObjectPropertyAddress(mSelector: kAudioProcessPropertyBundleID, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    var s: Unmanaged<CFString>?
    var sz = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    guard AudioObjectGetPropertyData(obj, &a, 0, nil, &sz, &s) == noErr, let s else { return "" }
    return s.takeRetainedValue() as String
}

func processObjects() -> [AudioObjectID] {
    var a = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyProcessObjectList, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    var sz: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &a, 0, nil, &sz) == noErr else { return [] }
    var ids = [AudioObjectID](repeating: 0, count: Int(sz) / 4)
    guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &a, 0, nil, &sz, &ids) == noErr else { return [] }
    return ids
}

struct Use: Equatable { var bundle: String; var pid: Int32; var input: Bool; var output: Bool }

func snapshot() -> [String: Use] {
    var m: [String: Use] = [:]
    for o in processObjects() {
        let input = prop(o, kAudioProcessPropertyIsRunningInput, UInt32(0)) != 0
        let output = prop(o, kAudioProcessPropertyIsRunningOutput, UInt32(0)) != 0
        if !input && !output { continue }
        let pid = prop(o, kAudioProcessPropertyPID, Int32(0))
        let b = bundleID(o)
        m["\(b)#\(pid)"] = Use(bundle: b, pid: pid, input: input, output: output)
    }
    return m
}

let args = CommandLine.arguments
let once = args.contains("--once")
var interval = 1.0
if let i = args.firstIndex(of: "--interval"), i + 1 < args.count, let v = Double(args[i + 1]) { interval = v }
let df = ISO8601DateFormatter()
func fmt(_ u: Use) -> String { "\(u.bundle.isEmpty ? "?" : u.bundle) pid=\(u.pid) mic=\(u.input ? "Y" : "-") out=\(u.output ? "Y" : "-")" }

var prev = snapshot()
if once {
    for u in prev.values.sorted(by: { $0.bundle < $1.bundle }) { print(fmt(u)) }
    exit(0)
}
print("# start \(df.string(from: Date())); active now: \(prev.count)")
for u in prev.values { print("  = \(fmt(u))") }
fflush(stdout)
while true {
    Thread.sleep(forTimeInterval: interval)
    let cur = snapshot()
    for (k, u) in cur where prev[k] != u { print("\(df.string(from: Date())) \(prev[k] == nil ? "+" : "~") \(fmt(u))") }
    for (k, u) in prev where cur[k] == nil { print("\(df.string(from: Date())) - \(fmt(u))") }
    prev = cur
    fflush(stdout)
}
