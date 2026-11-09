import Foundation
import Testing

@testable import SublateUI

struct LiveAppInstallerTests {
    let temp: TempFolder

    init() throws {
        temp = try TempFolder()
    }

    @Test func onlyAnAppInAWritableFolderCanReplaceItself() throws {
        let writable = try temp.folder("writable").appending(path: "Sublate.app")
        let readOnly = try temp.folder("read-only")
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: readOnly.filePath)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: readOnly.filePath) }

        #expect(LiveAppInstaller().canInstall(replacing: writable))
        #expect(!LiveAppInstaller().canInstall(replacing: readOnly.appending(path: "Sublate.app")))
    }

    @Test func anUntrustedBundleIsNotInstalled() async throws {
        let running = try FakeBundle.make(
            at: temp.folder("Applications").appending(path: "Sublate.app"), version: "0.1.0", marker: "old"
        )
        let staged = try FakeBundle.makeAdHocSigned(at: temp.folder("staging").appending(path: "Sublate.app"), version: "0.2.0")
        let oldFiles = FakeBundle.files(of: running)
        let installer = LiveAppInstaller(replacer: BundleReplacer(files: TestFileOperations(root: try temp.folder("system"))))

        await #expect(throws: AppInstallError.self) {
            try await installer.install(staged, version: "0.2.0", replacing: running)
        }

        #expect(FakeBundle.files(of: running) == oldFiles)
    }

    private func trustingInstaller() throws -> (LiveAppInstaller, running: URL, staged: URL) {
        let running = try FakeBundle.make(
            at: temp.folder("Applications").appending(path: "Sublate.app"), version: "0.1.0", marker: "old"
        )
        let staged = try FakeBundle.make(
            at: temp.folder("staging").appending(path: "Sublate.app"), version: "0.2.0", marker: "new"
        )
        let installer = LiveAppInstaller(
            verifier: BundleVerifier(signature: AcceptsAnySignature()),
            replacer: BundleReplacer(files: TestFileOperations(root: try temp.folder("system")))
        )
        return (installer, running, staged)
    }

    @Test func aTrustedBundleOfTheOfferedVersionTakesThePlaceOfTheRunningOne() async throws {
        let (installer, running, staged) = try trustingInstaller()

        try await installer.install(staged, version: "0.2.0", replacing: running)

        #expect(FakeBundle.marker(of: running) == "new")
        #expect(!FileManager.default.fileExists(atPath: staged.filePath))
    }

    @Test func aTrustedBundleOfAnotherVersionIsNotInstalled() async throws {
        let (installer, running, staged) = try trustingInstaller()
        let oldFiles = FakeBundle.files(of: running)

        await #expect(throws: AppInstallError.wrongVersion(expected: "0.3.0", found: "0.2.0")) {
            try await installer.install(staged, version: "0.3.0", replacing: running)
        }

        #expect(FakeBundle.files(of: running) == oldFiles)
    }
}
