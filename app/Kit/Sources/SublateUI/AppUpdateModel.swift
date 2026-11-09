import SublateCore
import Foundation
import Observation

/// Where the update stands, as far as the app knows yet.
enum AppUpdateState: Equatable {
    case unchecked
    case checking
    case upToDate
    case available(AppUpdate)
    /// `fraction` is `nil` until the first report and while the size is unknown.
    case downloading(AppUpdate, fraction: Double?)
    case installing(AppUpdate)
    /// The new version is in place, but a job started meanwhile, so the relaunch waits.
    case readyToRelaunch(AppUpdate)
    case installFailed(AppUpdate, String)
    /// The check itself failed.
    case failed(String)
}

/// Checks for a newer Sublate and, where the app can replace itself, installs it and relaunches.
@MainActor
@Observable
public final class AppUpdateModel {
    let currentVersion: String

    private(set) var state = AppUpdateState.unchecked

    private let backend: any AppUpdateBackend
    private let installer: any AppInstaller
    private let activity: any JobActivity
    private let runningApp: URL

    init(
        backend: any AppUpdateBackend,
        installer: any AppInstaller,
        activity: any JobActivity,
        currentVersion: String,
        runningApp: URL
    ) {
        self.backend = backend
        self.installer = installer
        self.activity = activity
        self.currentVersion = currentVersion
        self.runningApp = runningApp
    }

    /// An update is on offer, has an archive, and the running app's folder can be changed.
    var canInstall: Bool {
        guard case .available(let update) = state, update.archiveSha256 != nil else { return false }
        return installer.canInstall(replacing: runningApp)
    }

    /// A relaunch would stop the job the main window is running.
    var isBlockedByJob: Bool { activity.isBusy }

    var isBusy: Bool {
        switch state {
        case .checking, .downloading, .installing: true
        default: false
        }
    }

    var isAwaitingRelaunch: Bool {
        if case .readyToRelaunch = state { true } else { false }
    }

    /// The menu item and the Settings button both end up here, so only one check runs at a time.
    /// Once the new version is in place, the running one has nothing left to check.
    func check() async {
        guard !isBusy, !isAwaitingRelaunch else { return }
        state = .checking
        do {
            let update = try await backend.checkAppUpdate(currentVersion: currentVersion)
            state = update.map { .available($0) } ?? .upToDate
        } catch {
            state = .failed(error.userMessage)
        }
    }

    /// Installs exactly the update the last check offered, never another version.
    func install() async {
        guard canInstall, !isBlockedByJob, case .available(let update) = state else { return }
        state = .downloading(update, fraction: nil)
        do {
            let staged = try await tracking({ self.state = .downloading(update, fraction: $0) }) { [backend] channel in
                try await backend.downloadAppUpdate(update: update, listener: channel)
            }
            state = .installing(update)
            try await installer.install(URL(filePath: staged), version: update.version, replacing: runningApp)
            state = .readyToRelaunch(update)
            relaunch()
        } catch {
            state = .installFailed(update, error.userMessage)
        }
    }

    func relaunch() {
        guard isAwaitingRelaunch, !isBlockedByJob else { return }
        installer.relaunch(runningApp)
    }
}
