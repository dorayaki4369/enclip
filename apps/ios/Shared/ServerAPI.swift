import Foundation

/// 中継サーバーの HTTP API（apps/server を正とする）。
struct ServerAPI: Sendable {
    let baseURL: URL
    private let session: URLSession

    init(baseURL: URL = Config.serverURL) {
        self.baseURL = baseURL
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 15
        session = URLSession(configuration: configuration)
    }

    struct SeqItem: Decodable {
        let seq: UInt64
        let envelopeJson: String
    }

    enum APIError: LocalizedError {
        case status(Int)
        case response

        var errorDescription: String? {
            switch self {
            case let .status(code): "サーバーが \(code) を返しました"
            case .response: "サーバーの応答を読めませんでした"
            }
        }
    }

    // MARK: ペアリング

    func putJoin(groupId: Data, pairingId: Data, json: String) async throws {
        let request = makeRequest("PUT", groupId: groupId, path: "/pairings/\(encodeBase64url(bytes: pairingId))/join", token: nil, body: Data(json.utf8))
        try await expect(request, statuses: [201])
    }

    /// まだ書かれていなければ nil。
    func getSlot(groupId: Data, pairingId: Data, slot: String) async throws -> String? {
        let request = makeRequest("GET", groupId: groupId, path: "/pairings/\(encodeBase64url(bytes: pairingId))/\(slot)", token: nil, body: nil)
        let (data, response) = try await session.data(for: request)
        switch statusCode(response) {
        case 200: return String(decoding: data, as: UTF8.self)
        case 204: return nil
        case let code: throw APIError.status(code)
        }
    }

    // MARK: アイテム

    func postItem(group: GroupRecord, envelopeJson: String) async throws {
        let request = makeRequest("POST", groupId: group.groupId, path: "/items", token: group.accessToken, body: Data(envelopeJson.utf8))
        try await expect(request, statuses: [201])
    }

    func items(group: GroupRecord, after: UInt64) async throws -> [SeqItem] {
        var request = makeRequest("GET", groupId: group.groupId, path: "/items", token: group.accessToken, body: nil)
        var components = URLComponents(url: request.url!, resolvingAgainstBaseURL: false)!
        components.queryItems = [URLQueryItem(name: "after", value: String(after))]
        request.url = components.url
        let (data, response) = try await session.data(for: request)
        guard statusCode(response) == 200 else { throw APIError.status(statusCode(response)) }
        // 封筒はそのまま中核へ渡すので、JSON の形を保ったまま文字列に戻す。
        guard
            let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
            let items = root["items"] as? [[String: Any]]
        else { throw APIError.response }
        return try items.map { item in
            guard let seq = item["seq"] as? NSNumber, let envelope = item["envelope"] else { throw APIError.response }
            let envelopeData = try JSONSerialization.data(withJSONObject: envelope)
            return SeqItem(seq: seq.uint64Value, envelopeJson: String(decoding: envelopeData, as: UTF8.self))
        }
    }

    // MARK: 共通

    private func makeRequest(_ method: String, groupId: Data, path: String, token: Data?, body: Data?) -> URLRequest {
        let url = baseURL.appending(path: "groups/\(encodeBase64url(bytes: groupId))\(path)")
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.cachePolicy = .reloadIgnoringLocalCacheData
        if let token {
            request.setValue("Bearer \(encodeBase64url(bytes: token))", forHTTPHeaderField: "Authorization")
        }
        if let body {
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        return request
    }

    private func expect(_ request: URLRequest, statuses: Set<Int>) async throws {
        let (_, response) = try await session.data(for: request)
        let code = statusCode(response)
        guard statuses.contains(code) else { throw APIError.status(code) }
    }

    private func statusCode(_ response: URLResponse) -> Int {
        (response as? HTTPURLResponse)?.statusCode ?? 0
    }
}
