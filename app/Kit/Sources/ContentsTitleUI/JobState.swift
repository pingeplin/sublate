import Foundation

enum Status: Equatable {
    case idle
    case info(String)
    case failure(String)
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
    var subtitles: SubtitleProgress
}

struct OutputFile: Identifiable, Hashable {
    let label: String
    let url: URL

    var id: URL { url }
}
