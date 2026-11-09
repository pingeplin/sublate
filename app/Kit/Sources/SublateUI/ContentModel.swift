import SublateCore
import Foundation
import Observation

@MainActor
@Observable
public final class ContentModel: JobActivity {
    static let defaultTarget = "zh-TW"
    static let outputFolder = "Sublate"

    var urlText = ""
    var sourceTrack: SubtitleTrack?
    var targetCode = ContentModel.defaultTarget
    var outputDirectory: URL
    var content = ContentChoice.videoAndSubtitles
    /// `nil` takes the best the video offers.
    var maxHeight: UInt32?

    private(set) var status = Status.idle
    private(set) var languages: [TargetLanguage] = []
    private(set) var video: VideoMetadata?
    private(set) var progress: JobProgress?
    private(set) var files: [OutputFile] = []
    private(set) var isBusy = false

    private let backend: any ContentBackend

    init(backend: any ContentBackend, outputDirectory: URL) {
        self.backend = backend
        self.outputDirectory = outputDirectory
    }

    var canFetch: Bool {
        !isBusy && !urlText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Subtitles alone need a track to translate.
    var canGetContent: Bool {
        !isBusy && video != nil && (content.includesVideo || sourceTrack != nil)
    }

    func start() {
        languages = backend.targetLanguages()
    }

    func fetchMetadata() async {
        isBusy = true
        defer { isBusy = false }
        status = .info("Fetching metadata…")
        do {
            let fetched = try await backend.fetchMetadata(url: urlText)
            video = fetched
            sourceTrack = fetched.subtitles.first
            maxHeight = nil
            files = []
            progress = nil
            status = .idle
        } catch {
            video = nil
            status = .failure(error.userMessage)
        }
    }

    /// Downloads the video and translates the subtitle side by side, each only when the chosen
    /// content includes it; one failing doesn't stop the other.
    func getContent() async {
        guard canGetContent, let video else { return }
        let reference = video.reference
        let directory = outputDirectory.filePath
        let withVideo = content.includesVideo
        let height = maxHeight
        let track = content.includesSubtitles ? sourceTrack : nil
        let target = targetCode

        files = []
        progress = JobProgress(
            videoPercent: withVideo ? 0 : nil,
            subtitles: content.includesSubtitles ? (track == nil ? .unavailable : .downloading) : nil
        )
        isBusy = true
        defer { isBusy = false }
        status = .info("Working…")

        async let videoFailure = failure {
            if withVideo { try await downloadVideo(reference, maxHeight: height, to: directory) }
        }
        async let subtitleFailure = failure {
            if let track { try await translateSubtitles(reference, track: track, target: target, to: directory) }
        }
        let failures = await [videoFailure, subtitleFailure].compactMap { $0 }
        status = failures.isEmpty ? .info("Done.") : .failure(failures.joined(separator: "\n"))
    }

    private func downloadVideo(_ video: VideoRef, maxHeight: UInt32?, to directory: String) async throws {
        let path = try await tracking({ self.progress?.videoPercent = $0 }) { [backend] channel in
            try await backend.downloadVideo(video: video, outDir: directory, maxHeight: maxHeight, listener: channel)
        }
        files.append(OutputFile(label: "Video", url: URL(filePath: path)))
    }

    private func translateSubtitles(
        _ video: VideoRef,
        track: SubtitleTrack,
        target: String,
        to directory: String
    ) async throws {
        let output = try await tracking({ self.progress?.subtitles = $0 }) { [backend] channel in
            try await backend.translateSubtitles(
                video: video, outDir: directory, track: track, target: target, listener: channel
            )
        }
        files.append(OutputFile(label: "Source subtitle (\(track.code))", url: URL(filePath: output.sourcePath)))
        files.append(OutputFile(
            label: "Translated subtitle (\(target), \(output.cueCount) cues)",
            url: URL(filePath: output.translatedPath)
        ))
    }

    private func failure(of job: () async throws -> Void) async -> String? {
        do {
            try await job()
            return nil
        } catch {
            return error.userMessage
        }
    }
}
