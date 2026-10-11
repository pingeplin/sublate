import Foundation
import Testing

@testable import SublateUI

struct DefaultsAntProfileStoreTests {
    @Test func savesReplacesAndForgetsTheName() throws {
        let suite = "tech.radiw.sublate.tests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = DefaultsAntProfileStore(defaults: defaults)

        #expect(store.load() == nil)
        store.save("first")
        #expect(store.load() == "first")
        store.save("second")
        #expect(store.load() == "second")
        store.save(nil)
        #expect(store.load() == nil)
    }
}
