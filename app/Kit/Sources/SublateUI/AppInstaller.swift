import Foundation

/// Puts a downloaded Sublate in place of the running one.
protocol AppInstaller: Sendable {
    /// False when the folder that holds `running` is not writable: on the disk image, when macOS
    /// runs the app from a translocated copy, or in a folder the user may not change.
    func canInstall(replacing running: URL) -> Bool
    /// Verifies `staged`, then replaces `running` with it. On error, `running` is unchanged.
    func install(_ staged: URL, version: String, replacing running: URL) async throws
    /// Arranges for the app at `app` to start once this process has exited, then quits this process.
    @MainActor func relaunch(_ app: URL)
}

enum AppInstallError: LocalizedError, Equatable {
    case untrusted(String)
    case wrongVersion(expected: String, found: String?)
    case notRestored(copy: URL, home: URL)

    var errorDescription: String? {
        switch self {
        case .untrusted(let reason):
            "Sublate update failed: the downloaded app is not signed by Sublate's developer (\(reason))"
        case .wrongVersion(let expected, let found):
            "Sublate update failed: the downloaded app is version \(found ?? "unknown"), not \(expected)"
        case .notRestored(let copy, let home):
            "Sublate update failed, and the previous version could not be put back. It is at \(copy.filePath); move it to \(home.deletingLastPathComponent().filePath)."
        }
    }
}

struct LiveAppInstaller: AppInstaller {
    var verifier = BundleVerifier(signature: DeveloperIDSignature())
    var replacer = BundleReplacer(files: SystemFileOperations())

    func canInstall(replacing running: URL) -> Bool {
        FileManager.default.isWritableFile(atPath: running.deletingLastPathComponent().filePath)
    }

    func install(_ staged: URL, version: String, replacing running: URL) async throws {
        try verifier.verify(staged, version: version)
        try replacer.replace(running, with: staged)
    }

    @MainActor func relaunch(_ app: URL) {
        AppRelauncher.relaunch(app)
    }
}
