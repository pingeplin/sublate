import SublateUI
import SwiftUI

@main
struct SublateApp: App {
    @State private var model = ContentModel()

    var body: some Scene {
        Window("Sublate", id: "main") {
            ContentView(model: model)
                .frame(minWidth: 520, minHeight: 360)
        }
        .defaultSize(width: 640, height: 720)
    }
}
