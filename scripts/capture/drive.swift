import AppKit
import CoreGraphics
import Foundation
import IOKit

// Drives one Writ instance by pid. Every event is refused unless the harness
// instance is the frontmost app and the person at the machine has been idle
// for longer than IDLE_FLOOR seconds; the driver waits for both rather than
// posting into whatever else is on screen. Keys go to the pid's own event
// queue; clicks go through the HID tap and so need the app frontmost.
//
//   drive front                        -> "<pid>\t<name>" of the frontmost app
//   drive idle                         -> seconds since the last HID event
//   drive away                         -> exits 0 when nobody has touched the machine for IDLE_FLOOR s
//   drive stamp                        -> records that the harness itself just produced an event
//   drive windows <pid>                -> "<windowid>\t<x> <y> <w> <h>\t<title>" per on-screen window
//   drive activate <pid>               -> brings the app forward, then waits until it is frontmost
//   drive wait <pid>                   -> waits until frontmost and idle, prints nothing
//   drive key <pid> <name> [mods]      -> one key press; mods is a comma list of cmd,shift,alt,ctrl
//   drive type <pid> <text>            -> types text as unicode key events
//   drive click <pid> <x> <y> [right]  -> click at a screen point (top-left origin)
//   drive move <pid> <x> <y>           -> move the pointer (hover)
//   drive drag <pid> <x1> <y1> <x2> <y2> -> press, move, release
//   drive place <pid> <x> <y> <w> <h>   -> move and size the app's main window (accessibility API)

// Every event the driver posts resets the system's idle clock, so it keeps
// the time of its own last post here: an idle clock that runs from that
// moment means nobody else has touched the machine since.
let STAMP = ProcessInfo.processInfo.environment["CAPTURE_STAMP"] ?? NSTemporaryDirectory() + "writ-capture-drive.stamp"
let IDLE_FLOOR: Double = Double(ProcessInfo.processInfo.environment["CAPTURE_IDLE_FLOOR"] ?? "") ?? 45
let WAIT_LIMIT: Double = Double(ProcessInfo.processInfo.environment["CAPTURE_WAIT_LIMIT"] ?? "") ?? 900

let keyCodes: [String: CGKeyCode] = [
  "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11,
  "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21,
  "6": 22, "5": 23, "=": 24, "9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31,
  "u": 32, "[": 33, "i": 34, "p": 35, "l": 37, "j": 38, "'": 39, "k": 40, ";": 41, "\\": 42,
  ",": 43, "/": 44, "n": 45, "m": 46, ".": 47, "tab": 48, "space": 49, "`": 50, "delete": 51,
  "escape": 53, "return": 36, "left": 123, "right": 124, "down": 125, "up": 126, "home": 115,
  "end": 119, "pageup": 116, "pagedown": 121, "forwarddelete": 117,
]

func fail(_ message: String) -> Never {
  FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
  exit(2)
}

func frontmost() -> (pid_t, String) {
  guard let app = NSWorkspace.shared.frontmostApplication else { return (0, "") }
  return (app.processIdentifier, app.localizedName ?? "")
}

// The same number `ioreg -c IOHIDSystem` prints as HIDIdleTime, in seconds.
func idleSeconds() -> Double {
  let service = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("IOHIDSystem"))
  if service == 0 { return 0 }
  defer { IOObjectRelease(service) }
  guard let value = IORegistryEntryCreateCFProperty(service, "HIDIdleTime" as CFString, kCFAllocatorDefault, 0)?
    .takeRetainedValue() as? Int64
  else { return 0 }
  return Double(value) / 1_000_000_000
}

func stamp() {
  try? String(Date().timeIntervalSince1970).write(toFile: STAMP, atomically: true, encoding: .utf8)
}

// True when the machine has been idle for IDLE_FLOOR seconds, or when the only
// events since the person last went idle were the driver's own.
func personIsAway(_ idle: Double) -> Bool {
  if idle >= IDLE_FLOOR { return true }
  guard let text = try? String(contentsOfFile: STAMP, encoding: .utf8), let last = Double(text.trimmingCharacters(in: .whitespacesAndNewlines))
  else { return false }
  let sinceOwnPost = Date().timeIntervalSince1970 - last
  return sinceOwnPost < IDLE_FLOOR && idle + 0.3 >= sinceOwnPost
}

// Blocks until the instance is frontmost and nobody has touched the machine
// for IDLE_FLOOR seconds. Gives up after WAIT_LIMIT seconds so a run never
// hangs forever while someone works at the keyboard.
func waitReady(_ pid: pid_t) {
  let start = Date()
  var reported = false
  while true {
    let (front, name) = frontmost()
    let idle = idleSeconds()
    if front == pid && personIsAway(idle) { return }
    if Date().timeIntervalSince(start) > WAIT_LIMIT {
      fail("gave up after \(Int(WAIT_LIMIT)) s: frontmost is \(front) \(name), idle \(Int(idle)) s")
    }
    if !reported {
      FileHandle.standardError.write(
        "waiting: frontmost is \(front) \(name), idle \(Int(idle)) s (need pid \(pid) and \(Int(IDLE_FLOOR)) s)\n".data(using: .utf8)!)
      reported = true
    }
    if front != pid, let app = NSRunningApplication(processIdentifier: pid) {
      app.activate()
    }
    usleep(1_000_000)
  }
}

func flags(_ spec: String?) -> CGEventFlags {
  var f = CGEventFlags()
  for part in (spec ?? "").split(separator: ",") {
    switch part {
    case "cmd": f.insert(.maskCommand)
    case "shift": f.insert(.maskShift)
    case "alt", "option": f.insert(.maskAlternate)
    case "ctrl": f.insert(.maskControl)
    default: fail("unknown modifier \(part)")
    }
  }
  return f
}

func press(_ pid: pid_t, _ code: CGKeyCode, _ f: CGEventFlags) {
  waitReady(pid)
  let source = CGEventSource(stateID: .hidSystemState)
  guard let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true),
    let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false)
  else { fail("could not build key event") }
  down.flags = f
  up.flags = f
  down.postToPid(pid)
  usleep(20_000)
  up.postToPid(pid)
  stamp()
  usleep(40_000)
}

func mouse(_ type: CGEventType, _ point: CGPoint, _ button: CGMouseButton, _ source: CGEventSource?) -> CGEvent {
  guard let event = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: point, mouseButton: button)
  else { fail("could not build mouse event") }
  return event
}

let args = CommandLine.arguments.dropFirst()
guard let command = args.first else { fail("usage: drive <command> ...") }
let rest = Array(args.dropFirst())

switch command {
case "front":
  let (pid, name) = frontmost()
  print("\(pid)\t\(name)")

case "idle":
  print(String(format: "%.1f", idleSeconds()))

case "away":
  exit(personIsAway(idleSeconds()) ? 0 : 1)

case "stamp":
  stamp()

case "windows":
  guard rest.count >= 1, let pid = pid_t(rest[0]) else { fail("windows <pid>") }
  let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
  guard let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else { fail("no window list") }
  for info in list {
    guard let owner = info[kCGWindowOwnerPID as String] as? pid_t, owner == pid else { continue }
    let id = info[kCGWindowNumber as String] as? Int ?? 0
    let layer = info[kCGWindowLayer as String] as? Int ?? 0
    if layer != 0 { continue }
    let bounds = info[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let x = bounds["X"] as? Int ?? 0, y = bounds["Y"] as? Int ?? 0
    let w = bounds["Width"] as? Int ?? 0, h = bounds["Height"] as? Int ?? 0
    let title = info[kCGWindowName as String] as? String ?? ""
    print("\(id)\t\(x) \(y) \(w) \(h)\t\(title)")
  }

case "activate":
  guard rest.count >= 1, let pid = pid_t(rest[0]) else { fail("activate <pid>") }
  guard let app = NSRunningApplication(processIdentifier: pid) else { fail("no app with pid \(pid)") }
  app.activate()
  waitReady(pid)
  let (front, name) = frontmost()
  print("\(front)\t\(name)")

case "wait":
  guard rest.count >= 1, let pid = pid_t(rest[0]) else { fail("wait <pid>") }
  waitReady(pid)

case "key":
  guard rest.count >= 2, let pid = pid_t(rest[0]) else { fail("key <pid> <name> [mods]") }
  guard let code = keyCodes[rest[1].lowercased()] else { fail("unknown key \(rest[1])") }
  press(pid, code, flags(rest.count > 2 ? rest[2] : nil))

case "type":
  guard rest.count >= 2, let pid = pid_t(rest[0]) else { fail("type <pid> <text>") }
  let text = rest[1...].joined(separator: " ")
  let source = CGEventSource(stateID: .hidSystemState)
  for scalar in text.utf16 {
    waitReady(pid)
    var unit = scalar
    guard let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
      let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false)
    else { fail("could not build key event") }
    down.keyboardSetUnicodeString(stringLength: 1, unicodeString: &unit)
    up.keyboardSetUnicodeString(stringLength: 1, unicodeString: &unit)
    down.postToPid(pid)
    usleep(8_000)
    up.postToPid(pid)
    stamp()
    usleep(12_000)
  }

case "click", "move":
  guard rest.count >= 3, let pid = pid_t(rest[0]), let x = Double(rest[1]), let y = Double(rest[2]) else {
    fail("\(command) <pid> <x> <y> [right]")
  }
  waitReady(pid)
  let point = CGPoint(x: x, y: y)
  let right = rest.count > 3 && rest[3] == "right"
  let source = CGEventSource(stateID: .hidSystemState)
  mouse(.mouseMoved, point, .left, source).post(tap: .cghidEventTap)
  stamp()
  usleep(60_000)
  if command == "move" { break }
  waitReady(pid)
  let button: CGMouseButton = right ? .right : .left
  mouse(right ? .rightMouseDown : .leftMouseDown, point, button, source).post(tap: .cghidEventTap)
  usleep(40_000)
  mouse(right ? .rightMouseUp : .leftMouseUp, point, button, source).post(tap: .cghidEventTap)
  stamp()
  usleep(80_000)

case "drag":
  guard rest.count >= 5, let pid = pid_t(rest[0]), let x1 = Double(rest[1]), let y1 = Double(rest[2]),
    let x2 = Double(rest[3]), let y2 = Double(rest[4])
  else { fail("drag <pid> <x1> <y1> <x2> <y2>") }
  waitReady(pid)
  let source = CGEventSource(stateID: .hidSystemState)
  let from = CGPoint(x: x1, y: y1), to = CGPoint(x: x2, y: y2)
  mouse(.mouseMoved, from, .left, source).post(tap: .cghidEventTap)
  usleep(60_000)
  mouse(.leftMouseDown, from, .left, source).post(tap: .cghidEventTap)
  usleep(60_000)
  let steps = 12
  for i in 1...steps {
    let t = Double(i) / Double(steps)
    let p = CGPoint(x: x1 + (x2 - x1) * t, y: y1 + (y2 - y1) * t)
    mouse(.leftMouseDragged, p, .left, source).post(tap: .cghidEventTap)
    usleep(15_000)
  }
  mouse(.leftMouseUp, to, .left, source).post(tap: .cghidEventTap)
  stamp()
  usleep(80_000)

case "place":
  guard rest.count >= 5, let pid = pid_t(rest[0]), let x = Double(rest[1]), let y = Double(rest[2]),
    let w = Double(rest[3]), let h = Double(rest[4])
  else { fail("place <pid> <x> <y> <w> <h>") }
  if !AXIsProcessTrusted() { fail("this terminal has no accessibility access (System Settings > Privacy & Security > Accessibility)") }
  let app = AXUIElementCreateApplication(pid)
  var value: AnyObject?
  guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value) == .success,
    let windows = value as? [AXUIElement], let window = windows.first
  else { fail("pid \(pid) has no accessible window") }
  var origin = CGPoint(x: x, y: y)
  var size = CGSize(width: w, height: h)
  guard let originValue = AXValueCreate(.cgPoint, &origin), let sizeValue = AXValueCreate(.cgSize, &size)
  else { fail("could not build AX values") }
  if AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, originValue) != .success { fail("could not move the window") }
  if AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, sizeValue) != .success { fail("could not size the window") }
  stamp()
  usleep(300_000)

default:
  fail("unknown command \(command)")
}
