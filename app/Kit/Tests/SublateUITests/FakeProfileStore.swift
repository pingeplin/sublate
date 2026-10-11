@testable import SublateUI

final class FakeProfileStore: AntProfileStore {
    var name: String?

    func load() -> String? {
        name
    }

    func save(_ name: String?) {
        self.name = name
    }
}
