import ContentsTitleCore
import Foundation
import Testing

@testable import ContentsTitleUI

/// Drives the real Rust backend, so it hits YouTube and the Claude API. Run with `LIVE=1 swift test`.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["LIVE"] != nil))
struct LiveBridgeTests {
    @Test func fetchesDownloadsAndTranslatesAShortVideo() async {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = ContentModel(backend: Backend(), outputDirectory: directory)

        await model.start()
        model.urlText = "https://www.youtube.com/watch?v=jNQXAC9IVRw"
        await model.fetchMetadata()
        await model.getContent()

        #expect(model.credentialSource != nil)
        #expect(model.video?.title == "Me at the zoo")
        #expect(model.status == .info("Done."))
        #expect(model.progress?.videoPercent == 100)
        #expect(model.progress?.subtitles.fraction == 1)
        #expect(model.files.count == 3)
        #expect(model.files.allSatisfy { FileManager.default.fileExists(atPath: $0.url.filePath) })
    }
}
