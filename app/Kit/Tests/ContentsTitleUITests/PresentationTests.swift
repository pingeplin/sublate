import ContentsTitleCore
import Foundation
import Testing

@testable import ContentsTitleUI

struct PresentationTests {
    @Test(arguments: [(75.0, "1:15"), (59.6, "1:00"), (3725.0, "1:02:05")])
    func durationSwitchesToHoursPastOneHour(seconds: Double, text: String) {
        var video = VideoMetadata.sample()
        video.duration = seconds
        #expect(video.durationText == text)
    }

    @Test func durationIsOmittedWhenUnknown() {
        var video = VideoMetadata.sample()
        video.duration = nil
        #expect(video.durationText == nil)
    }

    @Test func subtitleProgressDescribesEachStage() {
        #expect(SubtitleProgress.unavailable.label == "No subtitles to translate")
        #expect(SubtitleProgress.downloading.label == "Downloading subtitles")
        #expect(SubtitleProgress.translating(done: 3, total: 12).label == "Translating 3/12 cues")
    }

    @Test func subtitleProgressFractionSurvivesAnEmptyTrack() {
        #expect(SubtitleProgress.downloading.fraction == 0)
        #expect(SubtitleProgress.translating(done: 3, total: 12).fraction == 0.25)
        #expect(SubtitleProgress.translating(done: 0, total: 0).fraction == 0)
    }

    @Test func backendErrorsShowTheirOwnMessage() {
        let error: any Error = BackendError.Failed(message: "yt-dlp failed: boom")
        #expect(error.userMessage == "yt-dlp failed: boom")
    }

    @Test func pathsUnderHomeAreAbbreviated() {
        let url = URL.homeDirectory.appending(path: "Downloads/contents title")
        #expect(url.abbreviatedPath == "~/Downloads/contents title")
    }
}
