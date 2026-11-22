import Foundation
import Security

/// 端末鍵とグループの秘密をキーチェーンに保存する。アプリとキーボード拡張で共有する。
enum SecretStore {
    private static let service = "net.dryk.enclip"
    private static let deviceKeyAccount = "device-key"
    private static let groupAccount = "group"

    /// 端末鍵。なければ作って保存する。
    static func deviceKey() throws -> Data {
        if let existing = try read(account: deviceKeyAccount) {
            return existing
        }
        let key = try generateDeviceKey()
        try write(account: deviceKeyAccount, data: key)
        return key
    }

    static func group() throws -> GroupRecord? {
        guard let data = try read(account: groupAccount) else { return nil }
        let stored = try JSONDecoder().decode(StoredGroup.self, from: data)
        return GroupRecord(groupId: stored.groupId, userKey: stored.userKey, accessToken: stored.accessToken)
    }

    static func saveGroup(_ group: GroupRecord) throws {
        let stored = StoredGroup(groupId: group.groupId, userKey: group.userKey, accessToken: group.accessToken)
        try write(account: groupAccount, data: JSONEncoder().encode(stored))
    }

    private struct StoredGroup: Codable {
        let groupId: Data
        let userKey: Data
        let accessToken: Data
    }

    private static func baseQuery(account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrAccessGroup as String: Config.appGroup,
        ]
    }

    private static func read(account: String) throws -> Data? {
        var query = baseQuery(account: account)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            return result as? Data
        case errSecItemNotFound:
            return nil
        default:
            throw KeychainError(status: status)
        }
    }

    private static func write(account: String, data: Data) throws {
        let query = baseQuery(account: account)
        let update = [kSecValueData as String: data]
        let status = SecItemUpdate(query as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            var insert = query
            insert[kSecValueData as String] = data
            // 端末のロック解除後に読める。キーボードは解除中にしか使わない。
            insert[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let addStatus = SecItemAdd(insert as CFDictionary, nil)
            guard addStatus == errSecSuccess else { throw KeychainError(status: addStatus) }
        } else if status != errSecSuccess {
            throw KeychainError(status: status)
        }
    }
}

struct KeychainError: LocalizedError {
    let status: OSStatus
    var errorDescription: String? { "キーチェーンのエラー（\(status)）" }
}
