import SublateCore
import Testing

@testable import SublateUI

@MainActor
struct AppUpdateModelTests {
    let backend = FakeBackend()
    let model: AppUpdateModel

    init() {
        model = AppUpdateModel(backend: backend, currentVersion: "0.1.0")
    }

    @Test func nothingIsCheckedUntilTheUserAsks() {
        #expect(model.state == .unchecked)
        #expect(backend.updateChecks.isEmpty)
    }

    @Test func aCheckReportsTheNewerVersion() async {
        backend.appUpdate = .success(.sample)

        await model.check()

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
        backend.appUpdate = .success(.sample)

        await model.check()

        #expect(model.state == .available(.sample))
    }

    @Test func aSecondCheckWhileOneRunsIsIgnored() async {
        async let first: Void = model.check()
        async let second: Void = model.check()
        _ = await (first, second)

        #expect(backend.updateChecks.count == 1)
        #expect(model.state == .upToDate)
    }
}
