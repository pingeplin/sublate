import SublateCore
import Foundation
import Testing

@testable import SublateUI

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

    @Test func ytdlpOffersADownloadUntilItIsInstalled() {
        #expect(YtdlpState(version: nil) == .missing)
        #expect(YtdlpState.missing.label == "Not installed")
        #expect(YtdlpState.missing.action == "Download")

        let installed = YtdlpState(version: "2026.09.02")
        #expect(installed.label == "2026.09.02")
        #expect(installed.action == "Check for Updates")
    }

    @Test func anAppUpdateCheckDescribesWhatItFound() {
        let update = AppUpdate(version: "0.2.0", pageUrl: "https://github.com/pingeplin/sublate/releases/tag/v0.2.0")
        #expect(AppUpdateState.unchecked.status == .idle)
        #expect(AppUpdateState.checking.status == .idle)
        #expect(AppUpdateState.upToDate.status == .info("Sublate is up to date."))
        #expect(AppUpdateState.available(update).status == .info("Version 0.2.0 is available."))
        #expect(AppUpdateState.failed("offline").status == .failure("offline"))
    }

    @Test func onlyANewerVersionOffersADownloadPage() {
        let update = AppUpdate(version: "0.2.0", pageUrl: "https://github.com/pingeplin/sublate/releases/tag/v0.2.0")
        #expect(AppUpdateState.available(update).downloadPage == URL(string: update.pageUrl))
        #expect(AppUpdateState.upToDate.downloadPage == nil)
        #expect(AppUpdateState.failed("offline").downloadPage == nil)
    }

    @Test func backendErrorsShowTheirOwnMessage() {
        let error: any Error = BackendError.Failed(message: "yt-dlp failed: boom")
        #expect(error.userMessage == "yt-dlp failed: boom")
    }

    @Test func pathsUnderHomeAreAbbreviated() {
        let url = URL.homeDirectory.appending(path: "Downloads/sublate videos")
        #expect(url.abbreviatedPath == "~/Downloads/sublate videos")
    }
}
