import Foundation
import Testing

@testable import SublateUI

struct BundleReplacerTests {
    let temp: TempFolder
    let running: URL
    let staged: URL

    init() throws {
        temp = try TempFolder()
        running = try FakeBundle.make(at: temp.folder("Applications").appending(path: "Sublate.app"), version: "0.1.0", marker: "old")
        staged = try FakeBundle.make(at: temp.folder("cache/app-update").appending(path: "Sublate.app"), version: "0.2.0", marker: "new")
        let longAgo = Date(timeIntervalSince1970: 1_600_000_000)
        try FileManager.default.setAttributes([.modificationDate: longAgo], ofItemAtPath: staged.filePath)
    }

    private func contents(of folder: URL) throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: folder.filePath).sorted()
    }

    @Test func theStagedBundleTakesThePlaceOfTheRunningOneWhichGoesToTheTrash() throws {
        let files = TestFileOperations(root: try temp.folder("system"))
        let started = Date.now.addingTimeInterval(-1)
        let oldFiles = FakeBundle.files(of: running)

        try BundleReplacer(files: files).replace(running, with: staged)

        #expect(FakeBundle.marker(of: running) == "new")
        #expect(try contents(of: running.deletingLastPathComponent()) == ["Sublate.app"])
        #expect(!FileManager.default.fileExists(atPath: staged.filePath))
        let trashed = try contents(of: files.trashFolder)
        #expect(trashed.count == 1)
        #expect(FakeBundle.files(of: files.trashFolder.appending(path: trashed[0])) == oldFiles)
        let modified = try FileManager.default.attributesOfItem(atPath: running.filePath)[.modificationDate] as? Date
        #expect(modified.map { $0 >= started } == true)
        #expect(try contents(of: files.workRoot).isEmpty)
    }

    @Test func theOldBundleIsPutBackWhenTheNewOneCannotTakeItsPlace() throws {
        let running = self.running
        let files = TestFileOperations(root: try temp.folder("system")) { source, destination in
            destination == running && source.lastPathComponent.hasPrefix("incoming-")
        }
        let oldFiles = FakeBundle.files(of: running)

        #expect(throws: CocoaError.self) {
            try BundleReplacer(files: files).replace(running, with: staged)
        }

        #expect(FakeBundle.files(of: running) == oldFiles)
        #expect(try contents(of: running.deletingLastPathComponent()) == ["Sublate.app"])
        #expect(!FileManager.default.fileExists(atPath: files.trashFolder.filePath))
    }

    @Test func nothingChangesWhenTheStagedBundleCannotBeMoved() throws {
        let staged = self.staged
        let files = TestFileOperations(root: try temp.folder("system")) { source, _ in source == staged }
        let oldFiles = FakeBundle.files(of: running)

        #expect(throws: CocoaError.self) {
            try BundleReplacer(files: files).replace(running, with: staged)
        }

        #expect(FakeBundle.files(of: running) == oldFiles)
        #expect(FakeBundle.marker(of: staged) == "new")
    }

    @Test func theOldBundleIsKeptAndNamedWhenItCannotBePutBack() throws {
        let running = self.running
        let files = TestFileOperations(root: try temp.folder("system")) { _, destination in destination == running }
        let oldFiles = FakeBundle.files(of: running)

        let error = #expect(throws: AppInstallError.self) {
            try BundleReplacer(files: files).replace(running, with: staged)
        }

        guard case .notRestored(let copy, let home) = error else {
            Issue.record("\(String(describing: error)) does not say where the old bundle is")
            return
        }
        #expect(FakeBundle.files(of: copy) == oldFiles)
        #expect(home == running)
        let message = error?.localizedDescription ?? ""
        #expect(message.contains(copy.filePath) && message.contains(running.deletingLastPathComponent().filePath))
    }

    @Test func theOldBundleIsKeptWhenItCannotGoToTheTrash() throws {
        let files = TestFileOperations(root: try temp.folder("system"), trashFails: true)
        let oldFiles = FakeBundle.files(of: running)

        try BundleReplacer(files: files).replace(running, with: staged)

        #expect(FakeBundle.marker(of: running) == "new")
        let kept = try contents(of: files.workRoot).flatMap { folder in
            try contents(of: files.workRoot.appending(path: folder)).map { files.workRoot.appending(path: "\(folder)/\($0)") }
        }
        #expect(kept.map(FakeBundle.files(of:)) == [oldFiles])
    }
}
