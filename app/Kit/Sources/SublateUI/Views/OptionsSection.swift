import SublateCore
import SwiftUI

struct OptionsSection: View {
    @Bindable var model: ContentModel
    let tracks: [SubtitleTrack]
    let heights: [UInt32]
    @State private var isChoosingFolder = false

    var body: some View {
        Section {
            Picker("Get", selection: $model.content) {
                ForEach(ContentChoice.allCases, id: \.self) { choice in
                    Text(choice.label).tag(choice)
                }
            }
            .pickerStyle(.segmented)

            Picker("Resolution", selection: $model.maxHeight) {
                Text("Best").tag(UInt32?.none)
                ForEach(heights, id: \.self) { height in
                    Text("\(height)p").tag(UInt32?.some(height))
                }
            }
            .disabled(!model.content.includesVideo)

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
            .disabled(tracks.isEmpty || !model.content.includesSubtitles)

            Picker("Target subtitle", selection: $model.targetCode) {
                ForEach(model.languages, id: \.code) { language in
                    Text(language.label).tag(language.code)
                }
            }
            .disabled(!model.content.includesSubtitles)

            LabeledContent("Save to") {
                Text(model.outputDirectory.abbreviatedPath)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(model.outputDirectory.filePath)
                Button("Choose…") { isChoosingFolder = true }
                    .disabled(model.isBusy)
            }
        } footer: {
            Button("Get Content") {
                Task { await model.getContent() }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(!model.canGetContent)
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
