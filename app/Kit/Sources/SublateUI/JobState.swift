import Foundation

enum Status: Equatable {
    case idle
    case info(String)
    case failure(String)
}

/// What one job fetches; a picker over these never lets the user ask for nothing.
enum ContentChoice: CaseIterable, Hashable {
    case videoAndSubtitles
    case video
    case subtitles

    var includesVideo: Bool { self != .subtitles }
    var includesSubtitles: Bool { self != .video }

    var label: String {
        switch self {
        case .videoAndSubtitles: "Video and subtitles"
        case .video: "Video only"
        case .subtitles: "Subtitles only"
        }
    }
}

enum SubtitleProgress: Equatable, Sendable {
    case unavailable
    case downloading
    case translating(done: Int, total: Int)

    var label: String {
        switch self {
        case .unavailable: "No subtitles to translate"
        case .downloading: "Downloading subtitles"
        case .translating(let done, let total): "Translating \(done)/\(total) cues"
        }
    }

    var fraction: Double {
        guard case .translating(let done, let total) = self else { return 0 }
        return Double(done) / Double(max(total, 1))
    }
}

struct JobProgress: Equatable {
    /// `nil` when the video isn't part of the job.
    var videoPercent: Double?
    /// `nil` when the subtitles aren't part of the job.
    var subtitles: SubtitleProgress?
}

struct OutputFile: Identifiable, Hashable {
    let label: String
    let url: URL

    var id: URL { url }
}
