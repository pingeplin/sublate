import SublateCore
import Foundation
import Observation

/// Where Claude's credential comes from, as far as the app knows yet.
enum CredentialState: Equatable {
    case checking
    case missing
    case available(source: String)
}

/// yt-dlp is downloaded rather than shipped, so a new installation starts without it.
enum YtdlpState: Equatable {
    case checking
    case missing
    case installed(version: String)

    init(version: String?) {
        self = version.map { .installed(version: $0) } ?? .missing
    }
}

@MainActor
@Observable
public final class SettingsModel {
    var apiKeyDraft = ""
    var antProfileDraft = ""

    private(set) var credential = CredentialState.checking
    private(set) var hasSavedKey = false
    private(set) var antProfile: String?
    private(set) var ytdlp = YtdlpState.checking
    private(set) var isUpdating = false
    private(set) var status = Status.idle

    private let backend: any SettingsBackend
    private let keyStore: any APIKeyStore
    private let profileStore: any AntProfileStore

    init(backend: any SettingsBackend, keyStore: any APIKeyStore, profileStore: any AntProfileStore) {
        self.backend = backend
        self.keyStore = keyStore
        self.profileStore = profileStore
    }

    var canSaveKey: Bool {
        !apiKeyDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var canSaveAntProfile: Bool {
        draftedAntProfile != antProfile
    }

    func start() async {
        use(antProfile: profileStore.load())
        do {
            try await use(savedKey: keyStore.load())
        } catch {
            status = .failure(error.userMessage)
            await refreshCredential()
        }
        await updateYtdlp(force: false)
    }

    func saveKey() async {
        let key = apiKeyDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        await applying {
            try keyStore.save(key)
            apiKeyDraft = ""
            await use(savedKey: key)
        }
    }

    func removeKey() async {
        await applying {
            try keyStore.remove()
            await use(savedKey: nil)
        }
    }

    /// An empty name leaves the choice to the shell, and then to `ant`.
    func saveAntProfile() async {
        let name = draftedAntProfile
        profileStore.save(name)
        use(antProfile: name)
        await refreshCredential()
    }

    /// Downloads yt-dlp the first time, and looks for a newer release after that.
    func installLatestYtdlp() async {
        await updateYtdlp(force: true)
    }

    /// Deletes the downloaded yt-dlp and the caches, which moving the app to the Trash would
    /// leave behind. The saved key is not part of it: `removeKey` deletes that.
    func clearData() async {
        await applying {
            try await backend.clearData()
            ytdlp = .missing
        }
    }

    private func applying(_ change: () async throws -> Void) async {
        do {
            try await change()
            status = .idle
        } catch {
            status = .failure(error.userMessage)
        }
    }

    private func use(savedKey key: String?) async {
        backend.setApiKey(key: key)
        hasSavedKey = key != nil
        await refreshCredential()
    }

    private var draftedAntProfile: String? {
        let name = antProfileDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        return name.isEmpty ? nil : name
    }

    private func use(antProfile name: String?) {
        backend.setAntProfile(name: name)
        antProfile = name
        antProfileDraft = name ?? ""
    }

    private func refreshCredential() async {
        credential = await backend.credentialSource().map { .available(source: $0) } ?? .missing
    }

    /// The routine check at launch stays quiet when it fails; one the user asked for reports back.
    private func updateYtdlp(force: Bool) async {
        isUpdating = true
        defer { isUpdating = false }
        do {
            ytdlp = YtdlpState(version: try await backend.ytdlpVersion())
            ytdlp = YtdlpState(version: try await backend.updateYtdlp(force: force))
            if force { status = .info("yt-dlp is up to date.") }
        } catch {
            if force || ytdlp == .checking { status = .failure(error.userMessage) }
        }
    }
}
