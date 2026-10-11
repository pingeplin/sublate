import SublateCore

/// What the main window asks of the backend.
protocol ContentBackend: Sendable {
    func targetLanguages() -> [TargetLanguage]
    func fetchMetadata(url: String) async throws -> VideoMetadata
    func downloadVideo(video: VideoRef, outDir: String, listener: VideoProgressListener) async throws -> String
    func translateSubtitles(
        video: VideoRef,
        outDir: String,
        track: SubtitleTrack,
        target: String,
        listener: TranslationProgressListener
    ) async throws -> SubtitleFiles
}

/// What Settings asks of the backend.
protocol SettingsBackend: Sendable {
    func setApiKey(key: String?)
    func credentialSource() async -> String?
    func ytdlpVersion() async throws -> String?
    func updateYtdlp(force: Bool) async throws -> String?
    func clearData() async throws
}

extension Backend: ContentBackend, SettingsBackend {}
