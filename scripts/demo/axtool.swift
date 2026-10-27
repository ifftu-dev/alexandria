// axtool PID dump | press LABEL [n] | fill LABEL TEXT | box LABEL
import ApplicationServices
import Foundation

let args = CommandLine.arguments
guard args.count >= 3, let pid = pid_t(args[1]) else {
    print("usage: axtool PID dump|press|fill|box ...")
    exit(2)
}
let cmd = args[2]
let app = AXUIElementCreateApplication(pid)
AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, kCFBooleanTrue)

func str(_ e: AXUIElement, _ a: String) -> String {
    var v: CFTypeRef?
    guard AXUIElementCopyAttributeValue(e, a as CFString, &v) == .success, let v else { return "" }
    if let s = v as? String { return s }
    if let n = v as? NSNumber { return n.stringValue }
    return ""
}

func children(_ e: AXUIElement) -> [AXUIElement] {
    var v: CFTypeRef?
    guard AXUIElementCopyAttributeValue(e, kAXChildrenAttribute as CFString, &v) == .success,
          let arr = v as? [AXUIElement] else { return [] }
    return arr
}

func frame(_ e: AXUIElement) -> String {
    var p: CFTypeRef?
    var s: CFTypeRef?
    AXUIElementCopyAttributeValue(e, kAXPositionAttribute as CFString, &p)
    AXUIElementCopyAttributeValue(e, kAXSizeAttribute as CFString, &s)
    var pt = CGPoint.zero
    var sz = CGSize.zero
    if let p { AXValueGetValue(p as! AXValue, .cgPoint, &pt) }
    if let s { AXValueGetValue(s as! AXValue, .cgSize, &sz) }
    return "\(Int(pt.x)),\(Int(pt.y)),\(Int(sz.width)),\(Int(sz.height))"
}

let shown: Set<String> = ["AXButton", "AXTextField", "AXSecureTextField", "AXLink", "AXHeading", "AXCheckBox",
                          "AXRadioButton", "AXPopUpButton", "AXComboBox", "AXMenuItem", "AXStaticText", "AXTextArea"]
let pressable: Set<String> = ["AXButton", "AXLink", "AXCheckBox", "AXRadioButton", "AXMenuItem", "AXPopUpButton", "AXStaticText"]
let fillable: Set<String> = ["AXTextField", "AXSecureTextField", "AXTextArea", "AXComboBox"]

var windows: CFTypeRef?
AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &windows)
guard let wins = windows as? [AXUIElement], let win = wins.first else {
    print("no window")
    exit(1)
}

let target = args.count > 3 ? args[3] : ""
let want = ((cmd == "press" || cmd == "box") && args.count > 4) ? Int(args[4]) ?? 1 : 1
var seen = 0
var stack = [win]
var out = ""
while !stack.isEmpty {
    let e = stack.removeLast()
    let role = str(e, "AXRole")
    let title = str(e, "AXTitle")
    let desc = str(e, "AXDescription")
    let ph = str(e, "AXPlaceholderValue")
    let value = str(e, "AXValue")
    if cmd == "dump" {
        if shown.contains(role) && (role != "AXStaticText" || title.count < 100) {
            out += "\(role) | \(title) | \(desc) | \(ph) | \(value.replacingOccurrences(of: "\n", with: " "))\n"
        }
    } else {
        let hit = title == target || desc == target || ph == target || (role == "AXStaticText" && value == target)
        if hit {
            if cmd == "press" && pressable.contains(role) {
                seen += 1
                if seen == want {
                    AXUIElementPerformAction(e, kAXPressAction as CFString)
                    print("pressed \(role) \(target)")
                    exit(0)
                }
            } else if cmd == "fill" && fillable.contains(role) {
                AXUIElementSetAttributeValue(e, kAXFocusedAttribute as CFString, kCFBooleanTrue)
                AXUIElementSetAttributeValue(e, kAXValueAttribute as CFString, args[4] as CFString)
                print("filled \(target)")
                exit(0)
            } else if cmd == "box" {
                seen += 1
                if seen == want {
                    print("\(frame(e)) \(role)")
                    exit(0)
                }
            }
        }
    }
    stack.append(contentsOf: children(e).reversed())
}
if cmd == "dump" { print(out, terminator: "") } else { print("NOT FOUND \(target)") }
