import SublateCore
import Foundation
import Testing

@testable import SublateUI

struct VideoProgressDisplayTests {
    private let bytes = ByteProgress(done: 1_100_000, total: 3_600_000)

    private func size(_ count: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: count, countStyle: .file)
    }

    private func shown(_ steps: (VideoStep, StepState)..., jobRunning: Bool = true) -> VideoProgressDisplay {
        VideoProgressDisplay(VideoProgress(steps: steps.map { VideoStepProgress(step: $0, state: $1) }), jobRunning: jobRunning)
    }

    @Test func aRunningStreamShowsItsPositionPercentAndSizes() {
        let display = shown((.video, .done), (.audio, .running(percent: 30, bytes: bytes)), (.finishing, .waiting))

        #expect(display.title == "Step 2 of 3 · Downloading audio stream")
        #expect(display.detail == "30% · \(size(1_100_000)) of \(size(3_600_000))")
        #expect(display.fraction == 0.3)
        #expect(display.segments.map(\.phase) == [.done, .running, .waiting])
        #expect(display.segments.map(\.label) == ["Video", "Audio", "Finishing"])
    }

    @Test func aSingleFileAndTheConversionHaveTheirOwnLabels() {
        let display = shown((.videoWithAudio, .running(percent: 10, bytes: nil)), (.finishing, .waiting), (.converting, .waiting))

        #expect(display.title == "Step 1 of 3 · Downloading video")
        #expect(display.segments.map(\.label) == ["Video", "Finishing", "Converting"])
    }

    @Test func anUnknownTotalShowsOnlyTheBytesDone() {
        let display = shown((.videoWithAudio, .running(percent: 30, bytes: ByteProgress(done: 1_100_000, total: nil))), (.finishing, .waiting))

        #expect(display.detail == "30% · \(size(1_100_000))")
    }

    @Test func thePercentIsRoundedDown() {
        let display = shown((.videoWithAudio, .running(percent: 29.9, bytes: nil)), (.finishing, .waiting))

        #expect(display.detail == "29%")
    }

    @Test func anUnknownPercentShowsTheSizesAndAnEmptyBar() {
        let display = shown((.videoWithAudio, .running(percent: nil, bytes: bytes)), (.finishing, .waiting))

        #expect(display.detail == "\(size(1_100_000)) of \(size(3_600_000))")
        #expect(display.fraction == nil)
    }

    @Test func finishingHasNoPercent() {
        let display = shown((.video, .done), (.audio, .done), (.finishing, .running(percent: nil, bytes: nil)))

        #expect(display.title == "Step 3 of 3 · Merging streams and adding cover art")
        #expect(display.detail == "Working…")
        #expect(display.fraction == nil)
    }

    @Test func theConversionChecksTheStreamsBeforeItReportsAPercent() {
        let checking = shown((.videoWithAudio, .done), (.finishing, .done), (.converting, .running(percent: nil, bytes: nil)))
        let converting = shown((.videoWithAudio, .done), (.finishing, .done), (.converting, .running(percent: 40, bytes: nil)))

        #expect(checking.title == "Step 3 of 3 · Checking streams for Apple devices")
        #expect(converting.title == "Step 3 of 3 · Converting for Apple devices")
    }

    @Test func aJobWithEveryStepDoneOrSkippedIsDone() {
        let display = shown((.videoWithAudio, .done), (.finishing, .done), (.converting, .skipped))

        #expect(display.title == "Done")
        #expect(display.detail == "")
        #expect(display.fraction == 1)
        #expect(display.segments.last?.label == "Not needed")
    }

    @Test func aFailedStepKeepsItsLastValues() {
        let display = shown((.video, .done), (.audio, .failed(percent: 30, bytes: bytes)), (.finishing, .waiting))

        #expect(display.title == "Step 2 of 3 · Failed: Downloading audio stream")
        #expect(display.detail == "30% · \(size(1_100_000)) of \(size(3_600_000))")
        #expect(display.fraction == 0.3)
        #expect(display.segments.map(\.phase) == [.done, .failed, .waiting])
    }

    @Test func aStepThatFailedBeforeItReportedShowsStoppedAndAnEmptyBar() {
        let display = shown((.video, .failed(percent: nil, bytes: nil)), (.audio, .waiting), (.finishing, .waiting))

        #expect(display.title == "Step 1 of 3 · Failed: Downloading video stream")
        #expect(display.detail == "Stopped")
        #expect(display.fraction == 0)
    }

    @Test func knownStepsThatHaveNotStartedAreStarting() {
        let display = shown((.video, .waiting), (.audio, .waiting), (.finishing, .waiting))

        #expect(display.title == "Starting…")
        #expect(display.detail == "")
        #expect(display.fraction == nil)
    }

    @Test func noStepsArePreparingWhileTheJobRunsAndStoppedAfter() {
        #expect(shown(jobRunning: true).title == "Preparing…")
        #expect(shown(jobRunning: true).fraction == nil)
        #expect(shown(jobRunning: false).title == "Stopped")
        #expect(shown(jobRunning: false).fraction == 0)
    }
}
