// Output loopback meter (macOS 14.4+): RMS of everything the default output device is sent,
// via a device-scoped process tap (muted audio does not reach the device, so a CATapMuted capture
// shows up as a drop). Prints {"rms":..,"db":..,"frames":..}.   usage: loopback <seconds>
import Foundation
import CoreAudio
import AudioToolbox

func prop(_ obj: AudioObjectID, _ sel: AudioObjectPropertySelector, _ def: UInt32) -> UInt32 {
    var a = AudioObjectPropertyAddress(mSelector: sel, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    var v = def; var sz = UInt32(MemoryLayout<UInt32>.size)
    AudioObjectGetPropertyData(obj, &a, 0, nil, &sz, &v); return v
}
let secs = Double(CommandLine.arguments.dropFirst().first ?? "3") ?? 3
let outDev: AudioObjectID = prop(AudioObjectID(kAudioObjectSystemObject), kAudioHardwarePropertyDefaultOutputDevice, 0)
var uidRef: Unmanaged<CFString>? = nil
var uidA = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyDeviceUID, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
var uidSz = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
AudioObjectGetPropertyData(outDev, &uidA, 0, nil, &uidSz, &uidRef)
let uid = (uidRef?.takeRetainedValue() as String?) ?? ""
let d = CATapDescription(__excludingProcesses: [], andDeviceUID: uid, withStream: 0)
d.isPrivate = true; d.muteBehavior = .unmuted; d.name = "LowkLoopback"
var tap = AudioObjectID(0)
guard AudioHardwareCreateProcessTap(d, &tap) == 0 else { print("{\"error\":\"tap\"}"); exit(2) }
let desc: [String: Any] = [
  kAudioAggregateDeviceNameKey: "LowkLoopback", kAudioAggregateDeviceUIDKey: UUID().uuidString,
  kAudioAggregateDeviceIsPrivateKey: true, kAudioAggregateDeviceTapAutoStartKey: true,
  kAudioAggregateDeviceMainSubDeviceKey: uid,
  kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: uid]],
  kAudioAggregateDeviceTapListKey: [[kAudioSubTapUIDKey: d.uuid.uuidString]]]
var agg = AudioObjectID(0)
guard AudioHardwareCreateAggregateDevice(desc as CFDictionary, &agg) == 0 else { print("{\"error\":\"agg\"}"); exit(2) }
var sum = 0.0, n = 0.0
var pid: AudioDeviceIOProcID?
AudioDeviceCreateIOProcIDWithBlock(&pid, agg, nil) { _, inp, _, _, _ in
  let l = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inp))
  for b in l { guard let p = b.mData?.assumingMemoryBound(to: Float.self) else { continue }
    let c = Int(b.mDataByteSize) / 4; for i in 0..<c { sum += Double(p[i] * p[i]) }; n += Double(c) } }
AudioDeviceStart(agg, pid)
Thread.sleep(forTimeInterval: secs)
AudioDeviceStop(agg, pid); AudioDeviceDestroyIOProcID(agg, pid!)
AudioHardwareDestroyAggregateDevice(agg); AudioHardwareDestroyProcessTap(tap)
let rms = n > 0 ? (sum / n).squareRoot() : 0
print("{\"rms\":\(rms),\"db\":\(rms > 0 ? 20 * log10(rms) : -200),\"frames\":\(Int(n))}")
