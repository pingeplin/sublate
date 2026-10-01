import ContentsTitleUI
import SwiftUI

@main
struct ContentsTitleApp: App {
    @State private var model = ContentModel()

    var body: some Scene {
        Window("Contents Title", id: "main") {
            ContentView(model: model)
                .frame(minWidth: 520, minHeight: 360)
        }
        .defaultSize(width: 640, height: 720)
    }
}
