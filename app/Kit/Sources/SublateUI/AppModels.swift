import SublateCore
import Foundation

/// The models behind the app's scenes, sharing one backend.
@MainActor
public struct AppModels {
    public let content: ContentModel
    public let settings: SettingsModel

    public init(bundle: Bundle = .main) {
        let identifier = bundle.bundleIdentifier ?? "tech.radiw.sublate"
        let backend = Backend(config: .installed(in: bundle, identifier: identifier))
        content = ContentModel(
            backend: backend,
            outputDirectory: URL.downloadsDirectory.appending(
                path: ContentModel.outputFolder, directoryHint: .isDirectory
            )
        )
        settings = SettingsModel(backend: backend, keyStore: KeychainAPIKeyStore(service: identifier))
    }
}

extension BackendConfig {
    /// The tools are sealed inside the bundle; whatever is written at run time goes under
    /// the user's Library.
    static func installed(in bundle: Bundle, identifier: String) -> BackendConfig {
        let resources = bundle.resourceURL ?? bundle.bundleURL
        return BackendConfig(
            toolsDir: resources.appending(path: "tools", directoryHint: .isDirectory).filePath,
            supportDir: URL.applicationSupportDirectory.appending(path: identifier).filePath,
            cacheDir: URL.cachesDirectory.appending(path: identifier).filePath
        )
    }
}
