import SwiftUI

public struct ContentView: View {
    private let model: ContentModel
    private let settings: SettingsModel

    public init(model: ContentModel, settings: SettingsModel) {
        self.model = model
        self.settings = settings
    }

    public var body: some View {
        Form {
            if settings.ytdlp == .missing {
                Section {
                    LabeledContent("Sublate fetches videos with yt-dlp, which isn't installed yet.") {
                        SettingsLink {
                            Text("Download in Settings…")
                        }
                    }
                }
            }
            URLSection(model: model)
            if let video = model.video {
                Section {
                    VideoSummary(video: video)
                }
                OptionsSection(model: model, tracks: video.subtitles)
                if let progress = model.progress {
                    ProgressSection(progress: progress)
                }
                if !model.files.isEmpty {
                    FilesSection(files: model.files)
                }
            }
        }
        .formStyle(.grouped)
        .navigationSubtitle(settings.credential.subtitle)
        .task {
            model.start()
            await settings.start()
        }
    }
}
