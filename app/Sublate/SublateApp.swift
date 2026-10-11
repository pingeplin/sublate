import SublateUI
import SwiftUI

@main
struct SublateApp: App {
    @State private var models = AppModels()

    var body: some Scene {
        Window("Sublate", id: "main") {
            ContentView(model: models.content, settings: models.settings)
                .frame(minWidth: 520, minHeight: 360)
        }
        .defaultSize(width: 640, height: 720)
        .commands {
            UpdateCommands(model: models.updates)
        }

        Settings {
            SettingsView(model: models.settings, updates: models.updates)
        }
    }
}
