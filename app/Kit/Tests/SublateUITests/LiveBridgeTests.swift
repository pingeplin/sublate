import SublateCore
import Foundation
import Testing

@testable import SublateUI

/// Drives the real Rust backend with the vendored tools (`make tools`), so it downloads yt-dlp
/// and hits YouTube and the Claude API. Run with `LIVE=1 swift test`.
@MainActor
@Suite(.enabled(if: ProcessInfo.processInfo.environment["LIVE"] != nil))
struct LiveBridgeTests {
    @Test func fetchesDownloadsAndTranslatesAShortVideo() async {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let backend = Backend(config: BackendConfig(
            toolsDir: Self.vendoredTools.filePath,
            supportDir: directory.appending(path: "support").filePath,
            cacheDir: directory.appending(path: "cache").filePath
        ))
        let settings = SettingsModel(backend: backend, keyStore: FakeKeyStore())
        let model = ContentModel(backend: backend, outputDirectory: directory.appending(path: "out"))

        await settings.start()
        #expect(settings.ytdlp == .missing)
        await settings.installLatestYtdlp()
        model.start()
        model.urlText = "https://www.youtube.com/watch?v=jNQXAC9IVRw"
        await model.fetchMetadata()
        await model.getContent()

        #expect(settings.credential != .missing)
        #expect(settings.ytdlp != .missing)
        #expect(model.video?.title == "Me at the zoo")
        #expect(model.status == .info("Done."))
        #expect(model.progress?.videoPercent == 100)
        #expect(model.progress?.subtitles.fraction == 1)
        #expect(model.files.count == 3)
        #expect(model.files.allSatisfy { FileManager.default.fileExists(atPath: $0.url.filePath) })
    }

    private static let vendoredTools = URL(filePath: #filePath)
        .appending(path: "../../../../../vendor/tools", directoryHint: .isDirectory)
        .standardizedFileURL
}
