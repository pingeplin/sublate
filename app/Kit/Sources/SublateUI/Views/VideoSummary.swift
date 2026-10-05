import SublateCore
import SwiftUI

struct VideoSummary: View {
    let video: VideoMetadata

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            if let thumbnail = video.thumbnail.flatMap(URL.init(string:)) {
                AsyncImage(url: thumbnail) { image in
                    image.resizable().aspectRatio(contentMode: .fill)
                } placeholder: {
                    Rectangle().fill(.quaternary)
                }
                .frame(width: 176, height: 99)
                .clipShape(.rect(cornerRadius: 8))
            }
            VStack(alignment: .leading, spacing: 4) {
                Text(video.title)
                    .font(.headline)
                    .textSelection(.enabled)
                if let duration = video.durationText {
                    Text(duration)
                        .font(.subheadline)
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}
