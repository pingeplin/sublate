import SublateCore
import Foundation
import Testing

@testable import SublateUI

@MainActor
struct AppUpdateModelTests {
    let backend = FakeBackend()
    let installer = FakeInstaller()
    let activity = FakeActivity()
    let running = URL(filePath: "/Applications/Sublate.app")
    let model: AppUpdateModel

    init() {
        model = AppUpdateModel(
            backend: backend, installer: installer, activity: activity, currentVersion: "0.1.0", runningApp: running
        )
    }

    private func offer(_ update: AppUpdate = .sample) async {
        backend.appUpdate = .success(update)
        await model.check()
    }

    /// Lets the model apply what the backend just reported, then returns the state it reached.
    private func settledState(after previous: AppUpdateState) async -> AppUpdateState {
        for attempt in 0..<100 where attempt < 10 || model.state == previous {
            await Task.yield()
        }
        return model.state
    }

    private var stagedApp: URL { URL(filePath: "/cache/app-update/Sublate.app") }

    // MARK: Checking

    @Test func nothingIsCheckedUntilTheUserAsks() {
        #expect(model.state == .unchecked)
        #expect(backend.updateChecks.isEmpty)
    }

    @Test func aCheckReportsTheNewerVersion() async {
        await offer()

        #expect(backend.updateChecks == ["0.1.0"])
        #expect(model.state == .available(.sample))
    }

    @Test func aCheckWithoutANewerVersionSaysSo() async {
        await model.check()

        #expect(model.state == .upToDate)
    }

    @Test func aFailedCheckShowsTheBackendMessage() async {
        backend.appUpdate = .failure(.Failed(message: "Sublate update check failed: offline"))

        await model.check()

        #expect(model.state == .failed("Sublate update check failed: offline"))
    }

    @Test func aLaterCheckReplacesTheEarlierResult() async {
        backend.appUpdate = .failure(.Failed(message: "Sublate update check failed: offline"))
        await model.check()

        await offer()

        #expect(model.state == .available(.sample))
    }

    @Test func aSecondCheckWhileOneRunsIsIgnored() async {
        async let first: Void = model.check()
        async let second: Void = model.check()
        _ = await (first, second)

        #expect(backend.updateChecks.count == 1)
        #expect(model.state == .upToDate)
    }

    // MARK: What is offered

    @Test func anUpdateWithAnArchiveInAWritableFolderCanBeInstalled() async {
        await offer()

        #expect(model.canInstall)
    }

    @Test func aReleaseWithoutAnArchiveOffersOnlyItsPage() async {
        await offer(.pageOnly)

        await model.install()

        #expect(!model.canInstall)
        #expect(model.state.downloadPage == URL(string: AppUpdate.sample.pageUrl))
        #expect(backend.appDownloads.isEmpty)
        #expect(model.state == .available(.pageOnly))
    }

    @Test func anAppThatCannotReplaceItselfOffersOnlyThePage() async {
        installer.installable = false
        await offer()

        await model.install()

        #expect(!model.canInstall)
        #expect(model.state.downloadPage == URL(string: AppUpdate.sample.pageUrl))
        #expect(backend.appDownloads.isEmpty)
    }

    @Test func nothingIsInstalledWithoutAnOffer() async {
        await model.check()

        await model.install()

        #expect(model.state == .upToDate)
        #expect(backend.appDownloads.isEmpty)
        #expect(installer.calls.isEmpty)
    }

    // MARK: Installing

    @Test func installingDownloadsTheCheckedUpdateReplacesTheAppThenRelaunches() async {
        await offer()

        await model.install()

        #expect(backend.appDownloads == [.sample])
        #expect(installer.calls == [
            .install(staged: stagedApp, version: "0.2.0", running: running),
            .relaunch(running),
        ])
        #expect(model.state == .readyToRelaunch(.sample))
    }

    @Test func theDownloadShowsItsProgressBeforeTheInstall() async {
        await offer()
        var seen: [AppUpdateState] = []
        backend.afterDownloadProgress = {
            seen.append(await settledState(after: seen.last ?? .downloading(.sample, fraction: nil)))
        }
        installer.duringInstall = { seen.append(model.state) }

        await model.install()

        #expect(seen == [
            .downloading(.sample, fraction: 0.5),
            .downloading(.sample, fraction: 1.0),
            .installing(.sample),
        ])
    }

    @Test func theStateIsIndeterminateBeforeTheFirstReport() async {
        await offer()
        backend.downloadProgress = [(1, 2)]
        var beforeTheReportApplies: AppUpdateState?
        backend.afterDownloadProgress = { beforeTheReportApplies = model.state }

        await model.install()

        #expect(beforeTheReportApplies == .downloading(.sample, fraction: nil))
    }

    @Test func aDownloadOfUnknownSizeIsIndeterminateAndStillInstalls() async {
        await offer()
        backend.downloadProgress = [(50, 100), (60, nil), (70, 0)]
        var seen: [AppUpdateState] = []
        backend.afterDownloadProgress = {
            seen.append(await settledState(after: seen.last ?? .downloading(.sample, fraction: nil)))
        }

        await model.install()

        #expect(seen == [
            .downloading(.sample, fraction: 0.5),
            .downloading(.sample, fraction: nil),
            .downloading(.sample, fraction: nil),
        ])
        #expect(installer.calls.last == .relaunch(running))
    }

    @Test func checkingOrInstallingAgainWhileAnInstallRunsIsIgnored() async {
        await offer()
        backend.downloadProgress = [(1, 2)]
        var statesKept: [Bool] = []
        let tryAgain = {
            let before = model.state
            await model.install()
            await model.check()
            statesKept.append(model.state == before)
        }
        backend.afterDownloadProgress = tryAgain
        installer.duringInstall = tryAgain

        await model.install()

        #expect(statesKept == [true, true])
        #expect(backend.appDownloads.count == 1)
        #expect(backend.updateChecks.count == 1)
        #expect(installer.calls.count { if case .install = $0 { true } else { false } } == 1)
    }

    @Test func aFailedDownloadShowsItsMessageAndThePageAndDoesNotRelaunch() async {
        await offer()
        backend.appDownload = .failure(.Failed(message: "Sublate update failed: release 0.2.0 does not match its checksum"))

        await model.install()

        let message = "Sublate update failed: release 0.2.0 does not match its checksum"
        #expect(model.state == .installFailed(.sample, message))
        #expect(model.state.status == .failure(message))
        #expect(model.state.downloadPage == URL(string: AppUpdate.sample.pageUrl))
        #expect(!model.canInstall)
        #expect(installer.calls.isEmpty)
    }

    @Test func aRejectedBundleShowsWhyAndDoesNotRelaunch() async {
        await offer()
        let rejection = AppInstallError.wrongVersion(expected: "0.2.0", found: "0.1.9")
        installer.installation = .failure(rejection)

        await model.install()
        await model.install()

        #expect(model.state == .installFailed(.sample, rejection.localizedDescription))
        #expect(rejection.localizedDescription.contains("0.1.9") && rejection.localizedDescription.contains("0.2.0"))
        #expect(!model.canInstall)
        #expect(installer.calls == [.install(staged: stagedApp, version: "0.2.0", running: running)])
    }

    // MARK: A job in the main window

    @Test func aRunningJobHoldsTheInstallBackUntilItEnds() async {
        await offer()
        activity.isBusy = true

        await model.install()

        #expect(model.isBlockedByJob)
        #expect(model.canInstall)
        #expect(model.state == .available(.sample))
        #expect(backend.appDownloads.isEmpty)
        #expect(installer.calls.isEmpty)

        activity.isBusy = false
        await model.install()

        #expect(!model.isBlockedByJob)
        #expect(installer.calls.last == .relaunch(running))
    }

    @Test func aJobThatStartsDuringTheInstallDelaysTheRelaunch() async {
        await offer()
        installer.duringInstall = { activity.isBusy = true }

        await model.install()

        #expect(model.state == .readyToRelaunch(.sample))
        #expect(!installer.calls.contains(.relaunch(running)))

        model.relaunch()
        #expect(!installer.calls.contains(.relaunch(running)))

        activity.isBusy = false
        model.relaunch()
        #expect(installer.calls.filter { $0 == .relaunch(running) }.count == 1)
    }

    @Test func onceTheNewVersionIsInPlaceThereIsNothingLeftToCheck() async {
        await offer()
        installer.duringInstall = { activity.isBusy = true }
        await model.install()

        await model.check()

        #expect(backend.updateChecks.count == 1)
        #expect(model.state == .readyToRelaunch(.sample))
    }

    @Test func aRelaunchIsOnlyPossibleOnceTheNewVersionIsInPlace() async {
        await offer()

        model.relaunch()

        #expect(installer.calls.isEmpty)
    }
}
