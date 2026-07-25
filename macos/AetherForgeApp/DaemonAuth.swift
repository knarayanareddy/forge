import Foundation
import Security

enum DaemonAuth {
    static let service = "AetherForge"
    static let account = "daemon-auth-token"

    static func loadToken() -> String? {
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
              let token = String(data: data, encoding: .utf8),
              !token.isEmpty
        else {
            return nil
        }
        return token
    }

    static func waitForToken(retries: Int = 30, delayMs: UInt64 = 100) async -> String? {
        for _ in 0 ..< retries {
            if let token = loadToken() {
                return token
            }
            try? await Task.sleep(nanoseconds: delayMs * 1_000_000)
        }
        return nil
    }
}
