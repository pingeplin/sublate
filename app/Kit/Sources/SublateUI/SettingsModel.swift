import SublateCore
import Foundation
import Observation

/// Where Claude's credential comes from, as far as the app knows yet.
enum CredentialState: Equatable {
    case checking
    case missing
    case available(source: String)
}

@MainActor
@Observable
public final class SettingsModel {
    var apiKeyDraft = ""

    private(set) var credential = CredentialState.checking
    private(set) var hasSavedKey = false
    private(set) var ytdlpVersion: String?
    private(set) var isUpdating = false
    private(set) var status = Status.idle

    private let backend: any BackendProtocol
    private let keyStore: any APIKeyStore

    init(backend: any BackendProtocol, keyStore: any APIKeyStore) {
        self.backend = backend
        self.keyStore = keyStore
    }

    var canSaveKey: Bool {
        !apiKeyDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    func start() async {
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
        await storing {
            try keyStore.save(key)
            apiKeyDraft = ""
            await use(savedKey: key)
        }
    }

    func removeKey() async {
        await storing {
            try keyStore.remove()
            await use(savedKey: nil)
        }
    }

    func checkForUpdates() async {
        await updateYtdlp(force: true)
    }

    private func storing(_ change: () async throws -> Void) async {
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

    private func refreshCredential() async {
        credential = await backend.credentialSource().map { .available(source: $0) } ?? .missing
    }

    /// The routine check at launch stays quiet when it fails; one the user asked for reports back.
    private func updateYtdlp(force: Bool) async {
        isUpdating = true
        defer { isUpdating = false }
        do {
            ytdlpVersion = try await backend.ytdlpVersion()
            ytdlpVersion = try await backend.updateYtdlp(force: force)
            if force { status = .info("yt-dlp is up to date.") }
        } catch {
            if force || ytdlpVersion == nil { status = .failure(error.userMessage) }
        }
    }
}
