import SwiftUI

struct ProgressSection: View {
    let video: VideoProgressDisplay?
    let subtitles: SubtitleProgress?

    var body: some View {
        Section("Progress") {
            if let video {
                VideoProgressRow(display: video)
            }
            if let subtitles {
                ProgressView(subtitles.label, value: subtitles.fraction)
            }
        }
    }
}

private struct VideoProgressRow: View {
    let display: VideoProgressDisplay

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !display.segments.isEmpty {
                HStack(alignment: .top, spacing: 6) {
                    ForEach(Array(display.segments.enumerated()), id: \.offset) { _, segment in
                        SegmentView(segment: segment)
                    }
                }
                .accessibilityElement(children: .combine)
            }
            HStack(alignment: .firstTextBaseline) {
                Text(display.title)
                Spacer()
                Text(display.detail)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            if let fraction = display.fraction {
                ProgressView(value: fraction)
            } else {
                ProgressView().progressViewStyle(.linear)
            }
        }
    }
}

private struct SegmentView: View {
    let segment: VideoProgressDisplay.Segment

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Capsule()
                .fill(color)
                .frame(height: 4)
            Text(segment.label)
                .font(.caption)
                .fontWeight(segment.phase == .running ? .semibold : .regular)
                .foregroundStyle(segment.phase == .running || segment.phase == .failed ? .primary : .secondary)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var color: Color {
        switch segment.phase {
        case .done: .accentColor
        case .running: .accentColor.opacity(0.5)
        case .failed: .red
        case .waiting, .skipped: .secondary.opacity(0.25)
        }
    }
}
