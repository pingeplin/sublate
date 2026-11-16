import SwiftUI

struct ProgressSection: View {
    let progress: JobProgress

    var body: some View {
        Section("Progress") {
            if let video = progress.video {
                ProgressView(video.label, value: video.percent, total: 100)
            }
            if let subtitles = progress.subtitles {
                ProgressView(subtitles.label, value: subtitles.fraction)
            }
        }
    }
}
