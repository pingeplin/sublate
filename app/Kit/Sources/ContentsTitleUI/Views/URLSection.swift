import SwiftUI

struct URLSection: View {
    @Bindable var model: ContentModel
    @FocusState private var isFocused: Bool

    var body: some View {
        Section {
            HStack {
                TextField("Video URL", text: $model.urlText, prompt: Text("Paste a video URL…"))
                    .labelsHidden()
                    .focused($isFocused)
                    .onSubmit(fetch)
                    .disabled(model.isBusy)
                Button("Fetch", action: fetch)
                    .disabled(!model.canFetch)
            }
        } footer: {
            if model.isBusy || model.status != .idle {
                StatusLine(status: model.status, isBusy: model.isBusy)
            }
        }
        // Focus requested synchronously on appear is ignored; one run-loop turn later it takes effect.
        .onAppear {
            DispatchQueue.main.async { isFocused = true }
        }
    }

    private func fetch() {
        guard model.canFetch else { return }
        Task { await model.fetchMetadata() }
    }
}

private struct StatusLine: View {
    let status: Status
    let isBusy: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            if isBusy {
                ProgressView().controlSize(.small)
            }
            switch status {
            case .idle:
                EmptyView()
            case .info(let message):
                Text(message)
            case .failure(let message):
                Text(message)
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
