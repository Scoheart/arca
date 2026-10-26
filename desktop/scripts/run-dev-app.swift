// Cargo runner for macOS: launch a distinct development .app through LaunchServices.
import AppKit
import Foundation
import Darwin

let desktop = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let tauri = desktop.appendingPathComponent("src-tauri")
let files = FileManager.default
func json(_ path: URL) throws -> [String: Any] {
    try JSONSerialization.jsonObject(with: Data(contentsOf: path)) as! [String: Any]
}
let base = try json(tauri.appendingPathComponent("tauri.conf.json"))
let development = try json(tauri.appendingPathComponent("tauri.dev.conf.json"))
let name = development["productName"] as! String
let identifier = development["identifier"] as! String
guard CommandLine.arguments.count >= 2 else { fatalError("A compiled development executable is required") }
let executable = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
let bundle = executable.deletingLastPathComponent().appendingPathComponent("\(name).app")
let contents = bundle.appendingPathComponent("Contents")
let macOS = contents.appendingPathComponent("MacOS")
let resources = contents.appendingPathComponent("Resources")
try files.createDirectory(at: macOS, withIntermediateDirectories: true)
try files.createDirectory(at: resources, withIntermediateDirectories: true)
let binary = macOS.appendingPathComponent("desktop")
let staged = macOS.appendingPathComponent(".desktop-\(UUID().uuidString)")
try files.copyItem(at: executable, to: staged)
if files.fileExists(atPath: binary.path) { try files.removeItem(at: binary) }
try files.moveItem(at: staged, to: binary)
let iconPaths = (development["bundle"] as! [String: Any])["icon"] as! [String]
let icon = resources.appendingPathComponent("icon.icns")
if files.fileExists(atPath: icon.path) { try files.removeItem(at: icon) }
try files.copyItem(at: tauri.appendingPathComponent(iconPaths.first { $0.hasSuffix(".icns") }!), to: icon)
var info = try PropertyListSerialization.propertyList(from: Data(contentsOf: tauri.appendingPathComponent("Info.plist")), format: nil) as! [String: Any]
info.merge([
    "CFBundleName": name, "CFBundleDisplayName": name,
    "CFBundleIdentifier": identifier, "CFBundleExecutable": "desktop",
    "CFBundleShortVersionString": base["version"] as! String,
    "CFBundleVersion": base["version"] as! String,
    "CFBundlePackageType": "APPL", "CFBundleIconFile": "icon.icns",
    "NSPrincipalClass": "NSApplication"
]) { _, value in value }
try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
    .write(to: contents.appendingPathComponent("Info.plist"), options: .atomic)
let signing = Process()
signing.executableURL = URL(fileURLWithPath: "/usr/bin/codesign")
signing.arguments = ["--force", "--sign", "-", "--entitlements", tauri.appendingPathComponent("Entitlements.plist").path, bundle.path]
try signing.run()
signing.waitUntilExit()
guard signing.terminationStatus == 0 else { exit(signing.terminationStatus) }

let configuration = NSWorkspace.OpenConfiguration()
configuration.createsNewApplicationInstance = true
configuration.arguments = Array(CommandLine.arguments.dropFirst(2))
// Preserve the actual dev environment without placing credentials in argv.
configuration.environment = ProcessInfo.processInfo.environment.filter { $0.key != "__CFBundleIdentifier" }
var application: NSRunningApplication?
var launchError: Error?
NSWorkspace.shared.openApplication(at: bundle, configuration: configuration) { app, error in
    application = app
    launchError = error
}
let deadline = Date().addingTimeInterval(20)
while application == nil && launchError == nil && Date() < deadline {
    RunLoop.current.run(until: Date().addingTimeInterval(0.1))
}
guard let application else {
    fputs("Could not launch development app: \(String(describing: launchError))\n", stderr)
    exit(1)
}
var stopping = false
var stopDeadline = Date.distantFuture
let signals = [SIGINT, SIGTERM].map { number -> DispatchSourceSignal in
    signal(number, SIG_IGN)
    let source = DispatchSource.makeSignalSource(signal: number, queue: .main)
    source.setEventHandler {
        stopping = true
        stopDeadline = Date().addingTimeInterval(3)
        application.terminate()
    }
    source.resume()
    return source
}
while !application.isTerminated {
    if stopping && Date() >= stopDeadline {
        kill(application.processIdentifier, SIGTERM)
        break
    }
    RunLoop.current.run(until: Date().addingTimeInterval(0.2))
}
withExtendedLifetime(signals) {}
