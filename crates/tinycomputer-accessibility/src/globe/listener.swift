import Cocoa
import Darwin

var fnIsDown = false
func emit(_ message: String) {
    FileHandle.standardOutput.write((message + "\n").data(using: .utf8)!)
    fflush(stdout)
}

guard let monitor = NSEvent.addGlobalMonitorForEvents(matching: .flagsChanged, handler: { event in
    let flags = event.modifierFlags
    let containsFn = flags.contains(.function)

    if containsFn && !fnIsDown {
        fnIsDown = true
        emit("FN_DOWN")
    } else if !containsFn && fnIsDown {
        fnIsDown = false
        emit("FN_UP")
    }

}) else {
    FileHandle.standardError.write("Failed to create event monitor\n".data(using: .utf8)!)
    exit(1)
}

let signalSource = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
signal(SIGTERM, SIG_IGN)
signalSource.setEventHandler {
    NSEvent.removeMonitor(monitor)
    exit(0)
}
signalSource.resume()

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
app.run()
