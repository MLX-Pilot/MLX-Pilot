import AppKit
import ApplicationServices
import ScreenCaptureKit
import AVFoundation

enum QAError: LocalizedError {
    case message(String)
    var errorDescription: String? {
        switch self { case .message(let detail): return detail }
    }
}

func emit(_ text: String) {
    print(text)
    fflush(stdout)
}

func application(at path: String) -> NSRunningApplication? {
    let expected = URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL.path
    return NSWorkspace.shared.runningApplications.first {
        $0.bundleURL?.resolvingSymlinksInPath().standardizedFileURL.path == expected
    }
}

func accessibleValue(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
    return value
}

func visit(_ element: AXUIElement, depth: Int = 0, _ body: (AXUIElement, String, String) -> Bool) -> Bool {
    guard depth < 38 else { return false }
    let role = accessibleValue(element, kAXRoleAttribute) as? String ?? ""
    var attributes = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute]
    if role == "AXStaticText" { attributes.append(kAXValueAttribute) }
    let names = attributes.compactMap {
        accessibleValue(element, $0) as? String
    }.filter { !$0.isEmpty }
    if body(element, role, names.first ?? "") { return true }
    let children = accessibleValue(element, kAXChildrenAttribute) as? [AXUIElement] ?? []
    return children.contains { visit($0, depth: depth + 1, body) }
}

func accessibility(_ args: [String]) throws {
    guard AXIsProcessTrusted() else { throw QAError.message("AX unavailable") }
    guard let app = application(at: args[1]) else { throw QAError.message("Bundle not running at requested path") }
    let root = AXUIElementCreateApplication(app.processIdentifier)
    AXUIElementSetAttributeValue(root, "AXManualAccessibility" as CFString, kCFBooleanTrue)
    AXUIElementSetAttributeValue(root, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
    let windows = accessibleValue(root, kAXWindowsAttribute) as? [AXUIElement] ?? []
    var webArea: AXUIElement?
    // ScreenCaptureKit adds an AX window for the sharing indicator while filming.
    for window in windows {
        if visit(window, { element, role, _ in
            guard role == "AXWebArea" else { return false }
            webArea = element
            return true
        }) { break }
    }
    guard let webArea else { throw QAError.message("No web view in the requested bundle") }
    var scope = webArea
    if args[0] == "press-in" {
        guard visit(webArea, { element, role, name in
            guard role == "AXTabGroup", name == args[2] else { return false }
            scope = element
            return true
        }) else { throw QAError.message("Requested tab group not found") }
    }
    let found = visit(scope) { element, role, name in
        let id = accessibleValue(element, "AXDOMIdentifier") as? String ?? ""
        switch args[0] {
        case "tree":
            if !name.isEmpty || !id.isEmpty || role == "AXWebArea" {
                emit("\(role) \(name)\(id.isEmpty ? "" : " [id=\(id)]")")
            }
            return false
        case "type-id":
            guard id == args[2], role == "AXTextArea" else { return false }
            _ = AXUIElementSetAttributeValue(element, kAXFocusedAttribute as CFString, kCFBooleanTrue)
            guard AXUIElementSetAttributeValue(element, kAXValueAttribute as CFString, args[3] as CFString) == .success,
                  accessibleValue(element, kAXValueAttribute) as? String == args[3] else { return false }
            emit("\(role) [id=\(id)]")
            return true
        case "press-id":
            guard id == args[2] else { return false }
        case "press-in":
            guard name == args[3], role == "AXRadioButton" || role == "AXButton" else { return false }
        default:
            guard role == args[2], name == args[3] else { return false }
        }
        guard AXUIElementPerformAction(element, kAXPressAction as CFString) == .success else { return false }
        emit("\(role) \(name)\(id.isEmpty ? "" : " [id=\(id)]")")
        return true
    }
    if args[0] != "tree" && !found { throw QAError.message("Requested accessibility control not found or press failed") }
}

final class RecordingEvents: NSObject, SCRecordingOutputDelegate {
    private let lock = NSLock()
    private var completed = false
    private var failure: Error?
    func recordingOutputDidStartRecording(_ recordingOutput: SCRecordingOutput) { emit("RECORDING") }
    func recordingOutputDidFinishRecording(_ recordingOutput: SCRecordingOutput) {
        lock.lock(); completed = true; lock.unlock()
    }
    func recordingOutput(_ recordingOutput: SCRecordingOutput, didFailWithError error: Error) {
        lock.lock(); failure = error; lock.unlock()
    }
    func finished() throws -> Bool {
        lock.lock(); defer { lock.unlock() }
        if let failure { throw failure }
        return completed
    }
}

func waitForWindow(_ path: String) async throws -> SCWindow {
    let deadline = Date().addingTimeInterval(120)
    repeat {
        if let app = application(at: path) {
            let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
            if let window = content.windows.first(where: {
                $0.owningApplication?.processID == app.processIdentifier && $0.windowLayer == 0 &&
                $0.frame.width >= 480 && $0.frame.height >= 300
            }) {
                emit("PID=\(app.processIdentifier) WINDOW=\(window.windowID) SIZE=\(window.frame.size)")
                return window
            }
        }
        try await Task.sleep(nanoseconds: 150_000_000)
    } while Date() < deadline
    throw QAError.message("Timed out awaiting window for exact bundle path")
}

@MainActor func record(_ args: [String]) async throws {
    guard CGPreflightScreenCaptureAccess() else { throw QAError.message("Screen capture permission unavailable") }
    let outputURL = URL(fileURLWithPath: args[2])
    guard !FileManager.default.fileExists(atPath: outputURL.path) else { throw QAError.message("Output already exists") }
    guard !FileManager.default.fileExists(atPath: args[3]) else { throw QAError.message("Stop marker already exists") }
    let duration = args.count > 4 ? Double(args[4]) ?? 180 : 180
    let window = try await waitForWindow(args[1])
    let filter = SCContentFilter(desktopIndependentWindow: window)
    let config = SCStreamConfiguration()
    config.width = Int(window.frame.width / 2) * 2
    config.height = Int(window.frame.height / 2) * 2
    config.minimumFrameInterval = CMTime(value: 1, timescale: 20)
    config.showsCursor = false
    config.ignoreShadowsSingleWindow = true
    config.capturesAudio = false
    let stream = SCStream(filter: filter, configuration: config, delegate: nil)
    let events = RecordingEvents()
    let recordingConfig = SCRecordingOutputConfiguration()
    recordingConfig.outputURL = outputURL
    recordingConfig.outputFileType = .mp4
    recordingConfig.videoCodecType = .h264
    let output = SCRecordingOutput(configuration: recordingConfig, delegate: events)
    try stream.addRecordingOutput(output)
    try await stream.startCapture()
    let deadline = Date().addingTimeInterval(duration)
    repeat {
        if try events.finished() { return }
        try await Task.sleep(nanoseconds: 200_000_000)
    } while Date() < deadline && !FileManager.default.fileExists(atPath: args[3])
    try await stream.stopCapture()
    for _ in 0..<60 {
        if try events.finished() { return }
        try await Task.sleep(nanoseconds: 200_000_000)
    }
    throw QAError.message("Video did not finish writing")
}

@main struct NativeQA {
    @MainActor static func main() async {
        do {
            _ = NSApplication.shared
            let args = Array(CommandLine.arguments.dropFirst())
            switch args.first {
            case "permissions":
                emit("ScreenCapture=\(CGPreflightScreenCaptureAccess()) Accessibility=\(AXIsProcessTrusted())")
            case "tree" where args.count == 2, "press" where args.count == 4,
                 "press-id" where args.count == 3, "press-in" where args.count == 4, "type-id" where args.count == 4:
                try accessibility(args)
            case "pid" where args.count == 2:
                if let app = application(at: args[1]) { emit(String(app.processIdentifier)) }
            case "quit" where args.count == 2:
                if let app = application(at: args[1]), !app.terminate() { throw QAError.message("Could not quit requested bundle") }
            case "record" where args.count >= 4: try await record(args)
            default: throw QAError.message("permissions | tree BUNDLE | press BUNDLE ROLE LABEL | press-id BUNDLE DOM_ID | press-in BUNDLE TAB_GROUP LABEL | type-id BUNDLE DOM_ID TEXT | record BUNDLE OUT.mp4 STOP_PATH [SECONDS]")
            }
        } catch {
            FileHandle.standardError.write(Data((error.localizedDescription + "\n").utf8))
            exit(1)
        }
    }
}
