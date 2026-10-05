import Foundation
import Security

/// Where the user's Anthropic API key rests between launches.
protocol APIKeyStore: Sendable {
    func load() throws -> String?
    func save(_ key: String) throws
    func remove() throws
}

struct KeychainError: LocalizedError, Equatable {
    let status: OSStatus

    var errorDescription: String? {
        let reason = SecCopyErrorMessageString(status, nil) as String? ?? "error \(status)"
        return "Keychain: \(reason)"
    }
}

/// A generic password in the login keychain. Access is tied to the app's Developer ID
/// signature, so updates keep reading the key without prompting.
struct KeychainAPIKeyStore: APIKeyStore {
    static let account = "anthropic-api-key"

    let service: String

    func load() throws -> String? {
        var query = item
        query[kSecReturnData] = true
        query[kSecMatchLimit] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        try check(status)
        return (result as? Data).flatMap { String(data: $0, encoding: .utf8) }
    }

    func save(_ key: String) throws {
        let value = [kSecValueData: Data(key.utf8)]
        let status = SecItemUpdate(item as CFDictionary, value as CFDictionary)
        if status == errSecItemNotFound {
            try check(SecItemAdd(item.merging(value) { $1 } as CFDictionary, nil))
        } else {
            try check(status)
        }
    }

    func remove() throws {
        let status = SecItemDelete(item as CFDictionary)
        if status != errSecItemNotFound { try check(status) }
    }

    private var item: [CFString: Any] {
        [kSecClass: kSecClassGenericPassword, kSecAttrService: service, kSecAttrAccount: Self.account]
    }

    private func check(_ status: OSStatus) throws {
        if status != errSecSuccess { throw KeychainError(status: status) }
    }
}
