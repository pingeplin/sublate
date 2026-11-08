import SwiftUI

struct ProgressSection: View {
    let progress: JobProgress

    var body: some View {
        Section("Progress") {
            if let percent = progress.videoPercent {
                ProgressView("Video", value: percent, total: 100)
            }
            if let subtitles = progress.subtitles {
                ProgressView(subtitles.label, value: subtitles.fraction)
            }
        }
    }
}
