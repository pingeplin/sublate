import SublateCore
import Foundation
import Observation

@MainActor
@Observable
public final class ContentModel {
    static let defaultTarget = "zh-TW"
    static let outputFolder = "Sublate"

    var urlText = ""
    var sourceTrack: SubtitleTrack?
    var targetCode = ContentModel.defaultTarget
    var outputDirectory: URL
    var downloadsVideo = true

    private(set) var status = Status.idle
    private(set) var credentialSource: String?
    private(set) var languages: [TargetLanguage] = []
    private(set) var video: VideoMetadata?
    private(set) var progress: JobProgress?
    private(set) var files: [OutputFile] = []
    private(set) var isBusy = false

    private let backend: any BackendProtocol

    init(backend: any BackendProtocol, outputDirectory: URL) {
        self.backend = backend
        self.outputDirectory = outputDirectory
    }

    public convenience init() {
        self.init(
            backend: Backend(),
            outputDirectory: URL.downloadsDirectory.appending(path: Self.outputFolder, directoryHint: .isDirectory)
        )
    }

    var canFetch: Bool {
        !isBusy && !urlText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    func start() async {
        languages = backend.targetLanguages()
        do {
            credentialSource = try await backend.credentialSource()
        } catch {
            status = .failure(error.userMessage)
        }
    }

    func fetchMetadata() async {
        isBusy = true
        defer { isBusy = false }
        status = .info("Fetching metadata…")
        do {
            let fetched = try await backend.fetchMetadata(url: urlText)
            video = fetched
            sourceTrack = fetched.subtitles.first
            files = []
            progress = nil
            status = .idle
        } catch {
            video = nil
            status = .failure(error.userMessage)
        }
    }

    /// Downloads the video and translates the subtitle side by side; one failing doesn't stop the other.
    func getContent() async {
        guard let video else { return }
        let reference = video.reference
        let directory = outputDirectory.filePath
        let track = sourceTrack
        let target = targetCode
        let withVideo = downloadsVideo

        files = []
        progress = JobProgress(
            videoPercent: withVideo ? 0 : nil,
            subtitles: track == nil ? .unavailable : .downloading
        )
        isBusy = true
        defer { isBusy = false }
        status = .info("Working…")

        async let videoFailure = failure {
            if withVideo { try await downloadVideo(reference, to: directory) }
        }
        async let subtitleFailure = failure {
            if let track { try await translateSubtitles(reference, track: track, target: target, to: directory) }
        }
        let failures = await [videoFailure, subtitleFailure].compactMap { $0 }
        status = failures.isEmpty ? .info("Done.") : .failure(failures.joined(separator: "\n"))
    }

    private func downloadVideo(_ video: VideoRef, to directory: String) async throws {
        let path = try await tracking({ self.progress?.videoPercent = $0 }) { [backend] channel in
            try await backend.downloadVideo(video: video, outDir: directory, listener: channel)
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

    /// Applies every progress value the operation reports before handing back its result.
    private func tracking<Value: Sendable, Output: Sendable>(
        _ apply: (Value) -> Void,
        during operation: @Sendable (ProgressChannel<Value>) async throws -> Output
    ) async throws -> Output {
        let channel = ProgressChannel<Value>()
        async let output = {
            defer { channel.finish() }
            return try await operation(channel)
        }()
        for await value in channel.values {
            apply(value)
        }
        return try await output
    }
}
