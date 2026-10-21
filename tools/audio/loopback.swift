// Acoustic output meter (macOS): RMS the default INPUT hears for <seconds>. With the built-in
// speakers playing a steady tone this is what "the user hears"; it drops to the noise floor when
// capture mutes playback (a device tap cannot see that: it observes the pre-mute mix).
// usage: loopback <seconds>   prints {"rms":..,"db":..}
import Foundation
import AVFoundation
let secs = Double(CommandLine.arguments.dropFirst().first ?? "3") ?? 3
let e = AVAudioEngine()
var sum = 0.0, n = 0.0
let lock = NSLock()
let inp = e.inputNode
inp.installTap(onBus: 0, bufferSize: 4096, format: inp.outputFormat(forBus: 0)) { b, _ in
  guard let p = b.floatChannelData?[0] else { return }
  lock.lock(); for i in 0..<Int(b.frameLength) { sum += Double(p[i] * p[i]) }; n += Double(b.frameLength); lock.unlock()
}
do { try e.start() } catch { print("{\"error\":\"engine\"}"); exit(2) }
Thread.sleep(forTimeInterval: secs)
e.stop()
let rms = n > 0 ? (sum / n).squareRoot() : 0
print("{\"rms\":\(rms),\"db\":\(rms > 0 ? 20 * log10(rms) : -200)}")
