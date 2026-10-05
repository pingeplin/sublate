import SwiftUI

public struct SettingsView: View {
    @Bindable private var model: SettingsModel

    public init(model: SettingsModel) {
        self.model = model
    }

    public var body: some View {
        Form {
            Section {
                LabeledContent("Using", value: model.credential.label)
                SecureField("Anthropic API key", text: $model.apiKeyDraft, prompt: Text(keyPrompt))
                    .onSubmit(save)
                HStack {
                    Spacer()
                    if model.hasSavedKey {
                        Button("Remove", role: .destructive) {
                            Task { await model.removeKey() }
                        }
                    }
                    Button("Save", action: save)
                        .disabled(!model.canSaveKey)
                }
            } header: {
                Text("Claude")
            } footer: {
                Text("Subtitles are translated with your key, which is kept in your Keychain.")
            }

            Section {
                LabeledContent("Version") {
                    if model.isUpdating {
                        ProgressView().controlSize(.small)
                    }
                    Text(model.ytdlp.label)
                    Button(model.ytdlp.action) {
                        Task { await model.installLatestYtdlp() }
                    }
                    .disabled(model.isUpdating)
                }
            } header: {
                Text("yt-dlp")
            } footer: {
                Text("Videos are fetched with yt-dlp, downloaded from its official releases and kept up to date.")
            }

            if model.status != .idle {
                Section {
                    StatusLine(status: model.status, isBusy: false)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 460)
        .fixedSize(horizontal: false, vertical: true)
    }

    private var keyPrompt: String {
        model.hasSavedKey ? "Saved — enter a new key to replace it" : "sk-ant-…"
    }

    private func save() {
        guard model.canSaveKey else { return }
        Task { await model.saveKey() }
    }
}
