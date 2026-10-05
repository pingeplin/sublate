import Foundation
import Testing

@testable import SublateUI

/// Writes to the real login keychain under a throwaway service name. Run with `LIVE=1 swift test`.
@Suite(.enabled(if: ProcessInfo.processInfo.environment["LIVE"] != nil))
struct KeychainAPIKeyStoreTests {
    @Test func savesReplacesAndRemovesTheKey() throws {
        let store = KeychainAPIKeyStore(service: "tech.radiw.sublate.tests.\(UUID().uuidString)")
        defer { try? store.remove() }

        #expect(try store.load() == nil)
        try store.save("sk-ant-first")
        #expect(try store.load() == "sk-ant-first")
        try store.save("sk-ant-second")
        #expect(try store.load() == "sk-ant-second")
        try store.remove()
        #expect(try store.load() == nil)
        try store.remove()
    }
}
