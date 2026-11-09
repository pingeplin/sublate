import Foundation

@testable import SublateUI

@MainActor
final class FakeInstaller: AppInstaller {
    enum Call: Equatable {
        case install(staged: URL, version: String, running: URL)
        case relaunch(URL)
    }

    var installable = true
    var installation: Result<Void, AppInstallError> = .success(())
    /// Runs while `install` is in progress.
    var duringInstall: () async -> Void = {}

    private(set) var calls: [Call] = []

    nonisolated func canInstall(replacing running: URL) -> Bool {
        MainActor.assumeIsolated { installable }
    }

    func install(_ staged: URL, version: String, replacing running: URL) async throws {
        calls.append(.install(staged: staged, version: version, running: running))
        await duringInstall()
        try installation.get()
    }

    func relaunch(_ app: URL) {
        calls.append(.relaunch(app))
    }
}

@MainActor
final class FakeActivity: JobActivity {
    var isBusy = false
}
