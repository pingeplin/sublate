import Foundation

/// The file system steps of a replacement, so a test can make one fail.
protocol FileOperations: Sendable {
    /// A new folder on the same volume as `url`, where moves are renames.
    func workFolder(appropriateFor url: URL) throws -> URL
    func move(_ source: URL, to destination: URL) throws
    func trash(_ url: URL) throws
    /// Lets Launch Services and Finder notice that the bundle changed.
    func markModified(_ url: URL) throws
    func remove(_ url: URL) throws
}

struct SystemFileOperations: FileOperations {
    func workFolder(appropriateFor url: URL) throws -> URL {
        try FileManager.default.url(for: .itemReplacementDirectory, in: .userDomainMask, appropriateFor: url, create: true)
    }

    /// Copies when the two are on different volumes.
    func move(_ source: URL, to destination: URL) throws {
        try FileManager.default.moveItem(at: source, to: destination)
    }

    func trash(_ url: URL) throws {
        try FileManager.default.trashItem(at: url, resultingItemURL: nil)
    }

    func markModified(_ url: URL) throws {
        try FileManager.default.setAttributes([.modificationDate: Date.now], ofItemAtPath: url.filePath)
    }

    func remove(_ url: URL) throws {
        try FileManager.default.removeItem(at: url)
    }
}

/// Swaps a bundle for a staged one. The old bundle goes to the Trash, so it can be recovered;
/// when a step fails, it is put back where it was. It is never deleted.
struct BundleReplacer: Sendable {
    let files: any FileOperations

    func replace(_ running: URL, with staged: URL) throws {
        let work = try files.workFolder(appropriateFor: running)
        let incoming = work.appending(path: "incoming-\(running.lastPathComponent)")
        let outgoing = work.appending(path: "outgoing-\(running.lastPathComponent)")
        do {
            try files.move(staged, to: incoming)
            try files.move(running, to: outgoing)
        } catch {
            try? files.remove(work)
            throw error
        }
        do {
            try files.move(incoming, to: running)
        } catch {
            try restore(outgoing, to: running)
            try? files.remove(work)
            throw error
        }
        // The new version is in place; neither step below can undo that.
        try? files.markModified(running)
        if (try? files.trash(outgoing)) != nil {
            try? files.remove(work)
        }
    }

    private func restore(_ outgoing: URL, to running: URL) throws {
        do {
            try files.move(outgoing, to: running)
        } catch {
            throw AppInstallError.notRestored(copy: outgoing, home: running)
        }
    }
}
