import SublateCore

@testable import SublateUI

@MainActor
final class FakeBackend: ContentBackend, SettingsBackend, AppUpdateBackend {
    nonisolated static let languages = [
        TargetLanguage(code: "zh-TW", native: "繁體中文（台灣）"),
        TargetLanguage(code: "en", native: "English"),
    ]

    /// What the shell offers when no key is saved in the app and no profile is chosen.
    var shellCredential: String? = "the active ant profile"
    var installedYtdlp: Result<String?, BackendError> = .success("2026.08.19")
    var latestYtdlp: Result<String, BackendError> = .success("2026.09.02")
    var clearing: Result<Void, BackendError> = .success(())
    var appUpdate: Result<AppUpdate?, BackendError> = .success(nil)
    var appDownload: Result<String, BackendError> = .success("/cache/app-update/Sublate.app")
    var downloadProgress: [(done: UInt64, total: UInt64?)] = [(50, 100), (100, 100)]
    /// Runs after each progress report, while the download is still in progress.
    var afterDownloadProgress: () async -> Void = {}
    var metadata: Result<VideoMetadata, BackendError> = .success(.sample())
    var video: Result<String, BackendError> = .success("/out/Title [abc].mp4")
    var subtitles: Result<SubtitleFiles, BackendError> = .success(
        SubtitleFiles(sourcePath: "/out/Title [abc].en.srt", translatedPath: "/out/Title [abc].zh-TW.srt", cueCount: 2)
    )
    var videoProgress: [Float] = [12.5, 100]
    var translationProgress: [(done: UInt64, total: UInt64)] = [(1, 2), (2, 2)]

    private(set) var apiKey: String?
    private(set) var antProfile: String?
    private(set) var updateRequests: [Bool] = []
    private(set) var clearRequests = 0
    private(set) var updateChecks: [String] = []
    private(set) var appDownloads: [AppUpdate] = []
    private(set) var fetchedURLs: [String] = []
    private(set) var videoRequests: [VideoRequest] = []
    private(set) var subtitleRequests: [SubtitleRequest] = []

    struct VideoRequest: Equatable {
        let video: VideoRef
        let outDir: String
        var maxHeight: UInt32?
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

    nonisolated func setAntProfile(name: String?) {
        MainActor.assumeIsolated { antProfile = name }
    }

    func credentialSource() async -> String? {
        if apiKey != nil { return "your saved API key" }
        return antProfile.map { "ant profile '\($0)'" } ?? shellCredential
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

    func clearData() async throws {
        clearRequests += 1
        try clearing.get()
        installedYtdlp = .success(nil)
    }

    /// Like the real backend, the answer takes a moment.
    func checkAppUpdate(currentVersion: String) async throws -> AppUpdate? {
        updateChecks.append(currentVersion)
        await Task.yield()
        return try appUpdate.get()
    }

    func downloadAppUpdate(update: AppUpdate, listener: AppDownloadListener) async throws -> String {
        appDownloads.append(update)
        for report in downloadProgress {
            listener.onDownloadProgress(done: report.done, total: report.total)
            await afterDownloadProgress()
        }
        return try appDownload.get()
    }

    func fetchMetadata(url: String) async throws -> VideoMetadata {
        fetchedURLs.append(url)
        return try metadata.get()
    }

    func downloadVideo(
        video: VideoRef,
        outDir: String,
        maxHeight: UInt32?,
        listener: VideoProgressListener
    ) async throws -> String {
        videoRequests.append(VideoRequest(video: video, outDir: outDir, maxHeight: maxHeight))
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

extension AppUpdate {
    static let sample = AppUpdate(
        version: "0.2.0",
        pageUrl: "https://github.com/pingeplin/sublate/releases/tag/v0.2.0",
        archiveSha256: "2b27b008b3a0888ec3fa29d6a6581467ad6b786db305e6800379a041756604f0"
    )

    /// A release that publishes no archive, only its page.
    static let pageOnly = AppUpdate(version: sample.version, pageUrl: sample.pageUrl, archiveSha256: nil)
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
            subtitles: subtitles,
            heights: [1080, 720, 360]
        )
    }
}
