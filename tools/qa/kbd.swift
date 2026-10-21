// QA helper for `just qa-bundle`: input sources (keyboard layouts) via the TIS API, and physical key presses.
//   kbd list                 selectable keyboard layouts (ids)
//   kbd current              id of the active layout
//   kbd select <id>          switch the active layout (the caller restores it)
//   kbd press <kVK> [cmd,shift,...]   post a key down/up with the given PHYSICAL key code (no characters)
import Carbon
import Foundation

func idOf(_ s: TISInputSource) -> String {
    guard let p = TISGetInputSourceProperty(s, kTISPropertyInputSourceID) else { return "" }
    return unsafeBitCast(p, to: CFString.self) as String
}
func layouts() -> [TISInputSource] {
    let f = [kTISPropertyInputSourceIsSelectCapable: true, kTISPropertyInputSourceCategory: kTISCategoryKeyboardInputSource] as CFDictionary
    return (TISCreateInputSourceList(f, false)?.takeRetainedValue() as? [TISInputSource]) ?? []
}
let a = CommandLine.arguments
switch a.count > 1 ? a[1] : "" {
case "list": layouts().forEach { print(idOf($0)) }
case "current": print(idOf(TISCopyCurrentKeyboardInputSource().takeRetainedValue()))
case "select":
    guard a.count > 2, let s = layouts().first(where: { idOf($0) == a[2] }) else { FileHandle.standardError.write(Data("no such layout\n".utf8)); exit(2) }
    exit(TISSelectInputSource(s) == noErr ? 0 : 3)
case "press":
    guard a.count > 2, let code = UInt16(a[2]) else { exit(2) }
    var flags: CGEventFlags = []
    for m in (a.count > 3 ? a[3] : "").split(separator: ",") {
        switch m { case "cmd": flags.insert(.maskCommand); case "shift": flags.insert(.maskShift); case "alt": flags.insert(.maskAlternate); case "ctrl": flags.insert(.maskControl); default: break }
    }
    let src = CGEventSource(stateID: .hidSystemState)
    for down in [true, false] {
        guard let e = CGEvent(keyboardEventSource: src, virtualKey: CGKeyCode(code), keyDown: down) else { exit(4) }
        e.flags = flags
        e.post(tap: .cghidEventTap)
        usleep(80_000)
    }
default: exit(1)
}
