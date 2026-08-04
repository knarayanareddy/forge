import Foundation
import Security

/// macOS Keychain storage for BYOK API keys (mirrors `aether_core::keychain` — no daemon IPC yet).
enum ByokKeychain {
    static let service = "AetherForge"
    static let account = "byok-api-key"

    static var isConfigured: Bool {
        loadKey() != nil
    }

    @discardableResult
    static func storeKey(_ apiKey: String) throws {
        let trimmed = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            throw ByokKeychainError.emptyKey
        }

        let data = Data(trimmed.utf8)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]

        let update: [String: Any] = [kSecValueData as String: data]
        let status = SecItemUpdate(query as CFDictionary, update as CFDictionary)
        if status == errSecSuccess {
            return
        }
        if status == errSecItemNotFound {
            var add = query
            add[kSecValueData as String] = data
            let addStatus = SecItemAdd(add as CFDictionary, nil)
            guard addStatus == errSecSuccess else {
                throw ByokKeychainError.access(addStatus)
            }
            return
        }
        throw ByokKeychainError.access(status)
    }

    static func deleteKey() throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw ByokKeychainError.access(status)
        }
    }

    private static func loadKey() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess,
              let data = item as? Data,
              let key = String(data: data, encoding: .utf8),
              !key.isEmpty
        else {
            return nil
        }
        return key
    }
}

enum ByokKeychainError: LocalizedError {
    case emptyKey
    case access(OSStatus)

    var errorDescription: String? {
        switch self {
        case .emptyKey:
            "API key cannot be empty."
        case .access(let status):
            "Keychain access failed (status \(status))."
        }
    }
}
