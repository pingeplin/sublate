import SublateCore
import Foundation
import Security
import Testing

@testable import SublateUI

@MainActor
struct SettingsModelTests {
    let backend = FakeBackend()
    let keyStore = FakeKeyStore()
    let model: SettingsModel

    init() {
        model = SettingsModel(backend: backend, keyStore: keyStore)
    }

    @Test func startHandsTheSavedKeyToTheBackend() async {
        keyStore.key = "sk-ant-saved"

        await model.start()

        #expect(backend.apiKey == "sk-ant-saved")
        #expect(model.hasSavedKey)
        #expect(model.credential == .available(source: "your saved API key"))
        #expect(model.status == .idle)
    }

    @Test func startWithoutASavedKeyFallsBackToTheShell() async {
        await model.start()

        #expect(backend.apiKey == nil)
        #expect(!model.hasSavedKey)
        #expect(model.credential == .available(source: "ant profile 'contents-title'"))
    }

    @Test func startWithoutAnyCredentialAsksForAKey() async {
        backend.shellCredential = nil

        await model.start()

        #expect(model.credential == .missing)
        #expect(model.credential.subtitle == "Add an Anthropic API key in Settings (⌘,)")
        #expect(model.status == .idle)
    }

    @Test func startReportsAnUnreadableKeychainAndStillResolvesACredential() async {
        keyStore.failure = KeychainError(status: errSecAuthFailed)

        await model.start()

        #expect(model.status == .failure(KeychainError(status: errSecAuthFailed).localizedDescription))
        #expect(model.credential == .available(source: "ant profile 'contents-title'"))
    }

    @Test func startSwitchesToANewerYtdlpWithoutForcingACheck() async {
        await model.start()

        #expect(backend.updateRequests == [false])
        #expect(model.ytdlp == .installed(version: "2026.09.02"))
        #expect(!model.isUpdating)
        #expect(model.status == .idle)
    }

    @Test func startWithoutYtdlpLeavesTheDownloadToTheUser() async {
        backend.installedYtdlp = .success(nil)

        await model.start()

        #expect(model.ytdlp == .missing)
        #expect(model.status == .idle)
    }

    @Test func downloadingInstallsTheLatestYtdlp() async {
        backend.installedYtdlp = .success(nil)
        await model.start()

        await model.installLatestYtdlp()

        #expect(backend.updateRequests == [false, true])
        #expect(model.ytdlp == .installed(version: "2026.09.02"))
        #expect(model.status == .info("yt-dlp is up to date."))
    }

    @Test func aFailedDownloadIsReportedAndYtdlpStaysMissing() async {
        backend.installedYtdlp = .success(nil)
        backend.latestYtdlp = .failure(.Failed(message: "yt-dlp update failed: offline"))

        await model.installLatestYtdlp()

        #expect(model.ytdlp == .missing)
        #expect(model.status == .failure("yt-dlp update failed: offline"))
    }

    @Test func aFailedRoutineUpdateKeepsTheInstalledVersionQuietly() async {
        backend.latestYtdlp = .failure(.Failed(message: "yt-dlp update failed: offline"))

        await model.start()

        #expect(model.ytdlp == .installed(version: "2026.08.19"))
        #expect(model.status == .idle)
    }

    @Test func anUnusableInstallationIsReportedAtStart() async {
        backend.installedYtdlp = .failure(.Failed(message: "the app's bundled tools are unusable: deno is missing"))

        await model.start()

        #expect(model.ytdlp == .checking)
        #expect(model.status == .failure("the app's bundled tools are unusable: deno is missing"))
    }

    @Test func checkingForUpdatesForcesTheCheckAndReportsBack() async {
        await model.installLatestYtdlp()

        #expect(backend.updateRequests == [true])
        #expect(model.ytdlp == .installed(version: "2026.09.02"))
        #expect(model.status == .info("yt-dlp is up to date."))
    }

    @Test func aFailedRequestedUpdateIsReported() async {
        backend.latestYtdlp = .failure(.Failed(message: "yt-dlp update failed: offline"))

        await model.installLatestYtdlp()

        #expect(model.ytdlp == .installed(version: "2026.08.19"))
        #expect(model.status == .failure("yt-dlp update failed: offline"))
    }

    @Test func savingStoresTheTrimmedKeyAndClearsTheField() async {
        model.apiKeyDraft = "  sk-ant-new\n"
        #expect(model.canSaveKey)

        await model.saveKey()

        #expect(keyStore.key == "sk-ant-new")
        #expect(backend.apiKey == "sk-ant-new")
        #expect(model.apiKeyDraft.isEmpty)
        #expect(model.hasSavedKey)
        #expect(model.credential == .available(source: "your saved API key"))
    }

    @Test func aBlankKeyCannotBeSaved() {
        model.apiKeyDraft = "  \n"
        #expect(!model.canSaveKey)
    }

    @Test func aKeyTheKeychainRejectsIsNotUsed() async {
        keyStore.failure = KeychainError(status: errSecAuthFailed)
        model.apiKeyDraft = "sk-ant-new"

        await model.saveKey()

        #expect(backend.apiKey == nil)
        #expect(!model.hasSavedKey)
        #expect(model.apiKeyDraft == "sk-ant-new")
        #expect(model.status == .failure(KeychainError(status: errSecAuthFailed).localizedDescription))
    }

    @Test func removingTheKeyFallsBackToTheShell() async {
        keyStore.key = "sk-ant-saved"
        await model.start()

        await model.removeKey()

        #expect(keyStore.key == nil)
        #expect(backend.apiKey == nil)
        #expect(!model.hasSavedKey)
        #expect(model.credential == .available(source: "ant profile 'contents-title'"))
    }
}
