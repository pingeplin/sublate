import AppKit
import SwiftUI

struct FilesSection: View {
    let files: [OutputFile]

    var body: some View {
        Section("Files") {
            ForEach(files) { file in
                LabeledContent {
                    Button("Show in Finder") {
                        NSWorkspace.shared.activateFileViewerSelecting([file.url])
                    }
                } label: {
                    Text(file.label)
                    Text(file.url.abbreviatedPath)
                        .textSelection(.enabled)
                }
            }
        }
    }
}
