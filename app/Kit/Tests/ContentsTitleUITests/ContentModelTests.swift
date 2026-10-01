import ContentsTitleCore
import Foundation
import Testing

@testable import ContentsTitleUI

@MainActor
struct ContentModelTests {
    let backend = FakeBackend()
    let model: ContentModel

    init() {
        model = ContentModel(backend: backend, outputDirectory: URL(filePath: "/out"))
    }

    private func fetched() async -> ContentModel {
        model.urlText = "https://example.com/watch?v=abc"
        await model.fetchMetadata()
        return model
    }

    @Test func startLoadsLanguagesAndCredentialSource() async {
        await model.start()

        #expect(model.languages == FakeBackend.languages)
        #expect(model.targetCode == "zh-TW")
        #expect(model.credentialSource == "ant profile contents-title")
        #expect(model.status == .idle)
    }

    @Test func startReportsACredentialFailure() async {
        backend.credential = .failure(.Failed(message: "cannot read the login shell environment: timeout"))

        await model.start()

        #expect(model.credentialSource == nil)
        #expect(model.status == .failure("cannot read the login shell environment: timeout"))
    }

    @Test func fetchCannotStartWithoutAURL() {
        #expect(!model.canFetch)
        model.urlText = "  \n"
        #expect(!model.canFetch)
        model.urlText = "https://example.com"
        #expect(model.canFetch)
    }

    @Test func fetchShowsTheVideoAndSelectsItsFirstTrack() async {
        let model = await fetched()

        #expect(backend.fetchedURLs == ["https://example.com/watch?v=abc"])
        #expect(model.video == .sample())
        #expect(model.sourceTrack == VideoMetadata.english)
        #expect(model.status == .idle)
        #expect(!model.isBusy)
    }

    @Test func fetchWithoutSubtitlesLeavesNoTrackSelected() async {
        backend.metadata = .success(.sample(subtitles: []))

        let model = await fetched()

        #expect(model.sourceTrack == nil)
    }

    @Test func fetchFailureHidesTheVideoAndShowsTheBackendMessage() async {
        _ = await fetched()
        backend.metadata = .failure(.Failed(message: "yt-dlp failed: boom"))

        await model.fetchMetadata()

        #expect(model.video == nil)
        #expect(model.status == .failure("yt-dlp failed: boom"))
        #expect(!model.isBusy)
    }

    @Test func fetchClearsThePreviousJob() async {
        let model = await fetched()
        await model.getContent()

        await model.fetchMetadata()

        #expect(model.files.isEmpty)
        #expect(model.progress == nil)
    }

    @Test func getContentDoesNothingBeforeAFetch() async {
        await model.getContent()

        #expect(backend.videoRequests.isEmpty)
        #expect(backend.subtitleRequests.isEmpty)
        #expect(model.progress == nil)
    }

    @Test func getContentDownloadsTheVideoAndTranslatesTheSelectedTrack() async {
        let model = await fetched()
        model.sourceTrack = VideoMetadata.koreanAuto
        model.targetCode = "en"
        let video = VideoRef(url: "https://example.com/watch?v=abc", id: "abc", title: "Title")

        await model.getContent()

        #expect(backend.videoRequests == [.init(video: video, outDir: "/out")])
        #expect(backend.subtitleRequests == [
            .init(video: video, outDir: "/out", track: VideoMetadata.koreanAuto, target: "en"),
        ])
        #expect(model.status == .info("Done."))
        #expect(!model.isBusy)
    }

    @Test func getContentListsEveryWrittenFile() async {
        let model = await fetched()

        await model.getContent()

        #expect(Set(model.files) == [
            OutputFile(label: "Video", url: URL(filePath: "/out/Title [abc].mp4")),
            OutputFile(label: "Source subtitle (en)", url: URL(filePath: "/out/Title [abc].en.srt")),
            OutputFile(label: "Translated subtitle (zh-TW, 2 cues)", url: URL(filePath: "/out/Title [abc].zh-TW.srt")),
        ])
    }

    @Test func getContentEndsOnTheLastReportedProgress() async {
        let model = await fetched()

        await model.getContent()

        #expect(model.progress == JobProgress(videoPercent: 100, subtitles: .translating(done: 2, total: 2)))
    }

    @Test func getContentSkipsTheVideoWhenNotWanted() async {
        let model = await fetched()
        model.downloadsVideo = false

        await model.getContent()

        #expect(backend.videoRequests.isEmpty)
        #expect(model.progress?.videoPercent == nil)
        #expect(model.files.map(\.label) == ["Source subtitle (en)", "Translated subtitle (zh-TW, 2 cues)"])
    }

    @Test func getContentWithoutATrackOnlyDownloadsTheVideo() async {
        backend.metadata = .success(.sample(subtitles: []))
        let model = await fetched()

        await model.getContent()

        #expect(backend.subtitleRequests.isEmpty)
        #expect(model.progress?.subtitles == .unavailable)
        #expect(model.files.map(\.label) == ["Video"])
        #expect(model.status == .info("Done."))
    }

    @Test func oneFailingJobDoesNotStopTheOther() async {
        backend.video = .failure(.Failed(message: "yt-dlp failed: no formats"))
        let model = await fetched()

        await model.getContent()

        #expect(model.status == .failure("yt-dlp failed: no formats"))
        #expect(model.files.map(\.label) == ["Source subtitle (en)", "Translated subtitle (zh-TW, 2 cues)"])
    }

    @Test func everyFailureIsReported() async {
        backend.video = .failure(.Failed(message: "yt-dlp failed: no formats"))
        backend.subtitles = .failure(.Failed(message: "Claude API error: overloaded"))
        let model = await fetched()

        await model.getContent()

        #expect(model.status == .failure("yt-dlp failed: no formats\nClaude API error: overloaded"))
        #expect(model.files.isEmpty)
    }
}
