import SwiftUI

struct ProgressSection: View {
    let progress: JobProgress

    var body: some View {
        Section("Progress") {
            if let percent = progress.videoPercent {
                ProgressView("Video", value: percent, total: 100)
            }
            ProgressView(progress.subtitles.label, value: progress.subtitles.fraction)
        }
    }
}
