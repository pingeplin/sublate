import SublateCore
import Observation

/// Whether a newer Sublate is published, as far as the app knows yet.
enum AppUpdateState: Equatable {
    case unchecked
    case checking
    case upToDate
    case available(AppUpdate)
    case failed(String)
}

/// The app only reports a newer version; the user downloads and installs it.
@MainActor
@Observable
public final class AppUpdateModel {
    let currentVersion: String

    private(set) var state = AppUpdateState.unchecked

    private let backend: any AppUpdateBackend

    init(backend: any AppUpdateBackend, currentVersion: String) {
        self.backend = backend
        self.currentVersion = currentVersion
    }

    var isChecking: Bool { state == .checking }

    /// The menu item and the Settings button both end up here, so only one check runs at a time.
    func check() async {
        guard !isChecking else { return }
        state = .checking
        do {
            let update = try await backend.checkAppUpdate(currentVersion: currentVersion)
            state = update.map { .available($0) } ?? .upToDate
        } catch {
            state = .failed(error.userMessage)
        }
    }
}
