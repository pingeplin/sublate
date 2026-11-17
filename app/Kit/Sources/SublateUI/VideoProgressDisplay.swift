import Foundation
import SublateCore

/// What the video row shows for one progress snapshot.
struct VideoProgressDisplay: Equatable {
    enum Phase: Equatable {
        case waiting, running, done, skipped, failed
    }

    struct Segment: Equatable {
        let label: String
        let phase: Phase
    }

    let segments: [Segment]
    let title: String
    let detail: String
    /// `nil` draws a bar with no value.
    let fraction: Double?

    /// `jobRunning` tells a job that has not named its steps yet from one that stopped first.
    init(_ progress: VideoProgress, jobRunning: Bool) {
        let steps = progress.steps
        segments = steps.map(Segment.init)

        if steps.isEmpty {
            (title, detail, fraction) = jobRunning ? ("Preparing…", "", nil) : ("Stopped", "", 0)
        } else if let index = segments.firstIndex(where: { $0.phase == .running || $0.phase == .failed }),
                  case let (percent, bytes)? = steps[index].state.reported {
            let failed = segments[index].phase == .failed
            let name = steps[index].step.runningTitle(percent: percent)
            title = "Step \(index + 1) of \(steps.count) · \(failed ? "Failed: " : "")\(name)"
            detail = Self.detail(percent: percent, bytes: bytes) ?? (failed ? "Stopped" : "Working…")
            fraction = percent.map { Double($0) / 100 } ?? (failed ? 0 : nil)
        } else if segments.allSatisfy({ $0.phase == .done || $0.phase == .skipped }) {
            (title, detail, fraction) = ("Done", "", 1)
        } else {
            (title, detail, fraction) = ("Starting…", "", nil)
        }
    }

    private static func detail(percent: Float?, bytes: ByteProgress?) -> String? {
        let parts = [percent.map { "\(Int($0))%" }, bytes.map(\.text)].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
}

extension VideoProgressDisplay.Segment {
    init(_ entry: VideoStepProgress) {
        let phase: VideoProgressDisplay.Phase = switch entry.state {
        case .waiting: .waiting
        case .running: .running
        case .done: .done
        case .skipped: .skipped
        case .failed: .failed
        }
        self.init(label: phase == .skipped ? "Not needed" : entry.step.label, phase: phase)
    }
}

extension VideoStep {
    var label: String {
        switch self {
        case .video, .videoWithAudio: "Video"
        case .audio: "Audio"
        case .finishing: "Finishing"
        case .converting: "Converting"
        }
    }

    func runningTitle(percent: Float?) -> String {
        switch self {
        case .video: "Downloading video stream"
        case .audio: "Downloading audio stream"
        case .videoWithAudio: "Downloading video"
        case .finishing: "Merging streams and adding cover art"
        case .converting: percent == nil ? "Checking streams for Apple devices" : "Converting for Apple devices"
        }
    }
}

extension StepState {
    /// The last values of a running or failed step.
    var reported: (percent: Float?, bytes: ByteProgress?)? {
        switch self {
        case .running(let percent, let bytes), .failed(let percent, let bytes): (percent, bytes)
        case .waiting, .done, .skipped: nil
        }
    }
}

extension ByteProgress {
    static func size(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }

    var text: String {
        guard let total else { return Self.size(done) }
        return "\(Self.size(done)) of \(Self.size(total))"
    }
}
