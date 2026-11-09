import AppKit

/// Opens the app again once this process is gone. `open` hands a bundle whose identifier is
/// still running to that process, so a helper outside the app waits for it to exit first.
enum AppRelauncher {
    private static let waitThenOpen = #"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$2""#

    @MainActor
    static func relaunch(_ app: URL) {
        let helper = Process()
        helper.executableURL = URL(filePath: "/bin/sh")
        helper.arguments = [
            "-c", waitThenOpen, "sublate-relaunch",
            String(ProcessInfo.processInfo.processIdentifier), app.filePath,
        ]
        do {
            try helper.run()
            NSApplication.shared.terminate(nil)
        } catch {
            openAlongside(app)
        }
    }

    /// Without the helper, the new version starts beside this one, which then quits.
    @MainActor
    private static func openAlongside(_ app: URL) {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: app, configuration: configuration) { _, error in
            guard error == nil else { return }
            Task { @MainActor in NSApplication.shared.terminate(nil) }
        }
    }
}
