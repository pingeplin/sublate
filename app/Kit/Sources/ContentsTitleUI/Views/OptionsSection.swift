import ContentsTitleCore
import SwiftUI

struct OptionsSection: View {
    @Bindable var model: ContentModel
    let tracks: [SubtitleTrack]
    @State private var isChoosingFolder = false

    var body: some View {
        Section {
            Picker("Source subtitle", selection: $model.sourceTrack) {
                if tracks.isEmpty {
                    Text("No subtitles available").tag(SubtitleTrack?.none)
                }
                ForEach(TrackKind.displayOrder, id: \.self) { kind in
                    let group = tracks.filter { $0.kind == kind }
                    if !group.isEmpty {
                        Section(kind.title) {
                            ForEach(group, id: \.self) { track in
                                Text(track.label).tag(SubtitleTrack?.some(track))
                            }
                        }
                    }
                }
            }
            .disabled(tracks.isEmpty)

            Picker("Target subtitle", selection: $model.targetCode) {
                ForEach(model.languages, id: \.code) { language in
                    Text(language.label).tag(language.code)
                }
            }

            LabeledContent("Save to") {
                Text(model.outputDirectory.abbreviatedPath)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(model.outputDirectory.filePath)
                Button("Choose…") { isChoosingFolder = true }
                    .disabled(model.isBusy)
            }

            Toggle("Download video", isOn: $model.downloadsVideo)
        } footer: {
            Button("Get Content") {
                Task { await model.getContent() }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(model.isBusy)
            .frame(maxWidth: .infinity, alignment: .trailing)
        }
        .fileImporter(isPresented: $isChoosingFolder, allowedContentTypes: [.folder]) { result in
            if case .success(let directory) = result {
                model.outputDirectory = directory
            }
        }
        .fileDialogDefaultDirectory(model.outputDirectory)
    }
}
