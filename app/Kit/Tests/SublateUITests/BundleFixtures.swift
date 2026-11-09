import Foundation

@testable import SublateUI

/// A folder of its own under the temporary directory, deleted with the value.
final class TempFolder {
    let url: URL

    init() throws {
        url = FileManager.default.temporaryDirectory.appending(path: "sublate-tests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    }

    deinit {
        try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.filePath)
        try? FileManager.default.removeItem(at: url)
    }

    func folder(_ path: String) throws -> URL {
        let folder = url.appending(path: path, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }
}

enum FakeBundle {
    /// A minimal Sublate.app whose executable holds `marker`; unsigned.
    @discardableResult
    static func make(at bundle: URL, version: String, marker: String) throws -> URL {
        let contents = bundle.appending(path: "Contents")
        try FileManager.default.createDirectory(at: contents.appending(path: "MacOS"), withIntermediateDirectories: true)
        let info: NSDictionary = [
            "CFBundleIdentifier": "tech.radiw.sublate",
            "CFBundleExecutable": "Sublate",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version,
        ]
        try info.write(to: contents.appending(path: "Info.plist"))
        try Data(marker.utf8).write(to: contents.appending(path: "MacOS/Sublate"))
        return bundle
    }

    /// The same, with a real executable signed ad hoc: valid, but by no one.
    static func makeAdHocSigned(at bundle: URL, version: String) throws -> URL {
        try make(at: bundle, version: version, marker: "")
        let executable = bundle.appending(path: "Contents/MacOS/Sublate")
        try FileManager.default.removeItem(at: executable)
        try FileManager.default.copyItem(at: URL(filePath: "/usr/bin/true"), to: executable)
        let sign = Process()
        sign.executableURL = URL(filePath: "/usr/bin/codesign")
        sign.arguments = ["--force", "--sign", "-", bundle.filePath]
        sign.standardError = FileHandle.nullDevice
        try sign.run()
        sign.waitUntilExit()
        precondition(sign.terminationStatus == 0, "ad hoc signing failed")
        return bundle
    }

    static func marker(of bundle: URL) -> String? {
        (try? Data(contentsOf: bundle.appending(path: "Contents/MacOS/Sublate"))).map { String(decoding: $0, as: UTF8.self) }
    }

    static func files(of bundle: URL) -> [String: Data] {
        let enumerator = FileManager.default.enumerator(at: bundle, includingPropertiesForKeys: nil)
        var files: [String: Data] = [:]
        while let file = enumerator?.nextObject() as? URL {
            if let data = try? Data(contentsOf: file) {
                files[String(file.filePath.dropFirst(bundle.filePath.count))] = data
            }
        }
        return files
    }
}

/// The system's operations, except that work folders and the Trash are inside a test folder,
/// and a move can be made to fail.
struct TestFileOperations: FileOperations {
    let root: URL
    var trashFails = false
    var failsToMove: @Sendable (_ source: URL, _ destination: URL) -> Bool = { _, _ in false }
    private let system = SystemFileOperations()

    init(root: URL, trashFails: Bool = false, failsToMove: @escaping @Sendable (URL, URL) -> Bool = { _, _ in false }) {
        self.root = root
        self.trashFails = trashFails
        self.failsToMove = failsToMove
    }

    var trashFolder: URL { root.appending(path: "Trash") }
    var workRoot: URL { root.appending(path: "work") }

    func workFolder(appropriateFor url: URL) throws -> URL {
        let folder = workRoot.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    func move(_ source: URL, to destination: URL) throws {
        if failsToMove(source, destination) { throw CocoaError(.fileWriteNoPermission) }
        try system.move(source, to: destination)
    }

    func trash(_ url: URL) throws {
        if trashFails { throw CocoaError(.fileWriteNoPermission) }
        try FileManager.default.createDirectory(at: trashFolder, withIntermediateDirectories: true)
        try system.move(url, to: trashFolder.appending(path: url.lastPathComponent))
    }

    func markModified(_ url: URL) throws {
        try system.markModified(url)
    }

    func remove(_ url: URL) throws {
        try system.remove(url)
    }
}

/// Stands in for the Developer ID check, which a test bundle cannot pass.
struct AcceptsAnySignature: SignatureCheck {
    func check(_ bundle: URL) throws {}
}
