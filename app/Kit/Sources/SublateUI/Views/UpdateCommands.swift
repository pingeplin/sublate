import SwiftUI

/// "Check for Updates…" in the app menu, below About. Settings shows what the check finds.
public struct UpdateCommands: Commands {
    private let model: AppUpdateModel
    @Environment(\.openSettings) private var openSettings

    public init(model: AppUpdateModel) {
        self.model = model
    }

    public var body: some Commands {
        CommandGroup(after: .appInfo) {
            Button("Check for Updates…") {
                openSettings()
                Task { await model.check() }
            }
        }
    }
}
