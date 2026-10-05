import SublateCore

@MainActor
final class FakeBackend: BackendProtocol {
    nonisolated static let languages = [
        TargetLanguage(code: "zh-TW", native: "繁體中文（台灣）"),
        TargetLanguage(code: "en", native: "English"),
    ]

    /// What the shell offers when no key is saved in the app.
    var shellCredential: String? = "ant profile 'contents-title'"
    var installedYtdlp: Result<String?, BackendError> = .success("2026.08.19")
    var latestYtdlp: Result<String, BackendError> = .success("2026.09.02")
    var metadata: Result<VideoMetadata, BackendError> = .success(.sample())
    var video: Result<String, BackendError> = .success("/out/Title [abc].mp4")
    var subtitles: Result<SubtitleFiles, BackendError> = .success(
        SubtitleFiles(sourcePath: "/out/Title [abc].en.srt", translatedPath: "/out/Title [abc].zh-TW.srt", cueCount: 2)
    )
    var videoProgress: [Float] = [12.5, 100]
    var translationProgress: [(done: UInt64, total: UInt64)] = [(1, 2), (2, 2)]

    private(set) var apiKey: String?
    private(set) var updateRequests: [Bool] = []
    private(set) var fetchedURLs: [String] = []
    private(set) var videoRequests: [VideoRequest] = []
    private(set) var subtitleRequests: [SubtitleRequest] = []

    struct VideoRequest: Equatable {
        let video: VideoRef
        let outDir: String
    }

    struct SubtitleRequest: Equatable {
        let video: VideoRef
        let outDir: String
        let track: SubtitleTrack
        let target: String
    }

    nonisolated func targetLanguages() -> [TargetLanguage] {
        Self.languages
    }

    nonisolated func setApiKey(key: String?) {
        MainActor.assumeIsolated { apiKey = key }
    }

    func credentialSource() async -> String? {
        apiKey == nil ? shellCredential : "your saved API key"
    }

    func ytdlpVersion() async throws -> String? {
        try installedYtdlp.get()
    }

    /// Like the real backend, the first release is only downloaded when forced.
    func updateYtdlp(force: Bool) async throws -> String? {
        updateRequests.append(force)
        if try installedYtdlp.get() == nil && !force { return nil }
        return try latestYtdlp.get()
    }

    func fetchMetadata(url: String) async throws -> VideoMetadata {
        fetchedURLs.append(url)
        return try metadata.get()
    }

    func downloadVideo(video: VideoRef, outDir: String, listener: VideoProgressListener) async throws -> String {
        videoRequests.append(VideoRequest(video: video, outDir: outDir))
        videoProgress.forEach(listener.onVideoProgress)
        return try self.video.get()
    }

    func translateSubtitles(
        video: VideoRef,
        outDir: String,
        track: SubtitleTrack,
        target: String,
        listener: TranslationProgressListener
    ) async throws -> SubtitleFiles {
        subtitleRequests.append(SubtitleRequest(video: video, outDir: outDir, track: track, target: target))
        translationProgress.forEach(listener.onTranslationProgress)
        return try subtitles.get()
    }
}

extension VideoMetadata {
    static let english = SubtitleTrack(code: "en", name: "English", kind: .manual)
    static let koreanAuto = SubtitleTrack(code: "ko-orig", name: "Korean (Original)", kind: .auto)

    static func sample(subtitles: [SubtitleTrack] = [english, koreanAuto]) -> VideoMetadata {
        VideoMetadata(
            id: "abc",
            title: "Title",
            thumbnail: "https://example.com/t.jpg",
            duration: 75,
            url: "https://example.com/watch?v=abc",
            subtitles: subtitles
        )
    }
}
