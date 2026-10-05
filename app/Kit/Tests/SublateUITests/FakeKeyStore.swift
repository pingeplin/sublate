import Foundation

@testable import SublateUI

final class FakeKeyStore: APIKeyStore, @unchecked Sendable {
    var key: String?
    var failure: KeychainError?

    init(key: String? = nil) {
        self.key = key
    }

    func load() throws -> String? {
        try failIfBroken()
        return key
    }

    func save(_ key: String) throws {
        try failIfBroken()
        self.key = key
    }

    func remove() throws {
        try failIfBroken()
        key = nil
    }

    private func failIfBroken() throws {
        if let failure { throw failure }
    }
}
