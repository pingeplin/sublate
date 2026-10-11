import Foundation

/// Where the name of the user's `ant` profile rests between launches.
protocol AntProfileStore {
    func load() -> String?
    func save(_ name: String?)
}

/// The name is no secret, so it lives in the app's preferences rather than the Keychain.
struct DefaultsAntProfileStore: AntProfileStore {
    static let key = "antProfile"

    let defaults: UserDefaults

    func load() -> String? {
        defaults.string(forKey: Self.key)
    }

    func save(_ name: String?) {
        defaults.set(name, forKey: Self.key)
    }
}
