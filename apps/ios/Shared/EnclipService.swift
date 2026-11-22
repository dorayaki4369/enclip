import Foundation

/// ペアリング、送信、受信をまとめる。暗号と検証は Rust の中核（enclip_ffi）が行う。
struct EnclipService: Sendable {
    let api: ServerAPI

    init(api: ServerAPI = ServerAPI()) {
        self.api = api
    }

    var isPaired: Bool {
        (try? SecretStore.group()) != nil
    }

    // MARK: ペアリング（新端末側、仕様 4.1）

    /// QR コードを読んで参加を申請する。
    func startJoin(qrText: String) async throws -> JoinFlow {
        let flow = try JoinFlow.scan(deviceKey: SecretStore.deviceKey(), qrText: qrText.trimmingCharacters(in: .whitespacesAndNewlines))
        try await api.putJoin(groupId: flow.groupId(), pairingId: flow.pairingId(), json: flow.joinRequestJson())
        return flow
    }

    /// 既存端末からの承認用ノンスを待ち、確認コードを返す。
    func waitForCode(_ flow: JoinFlow) async throws -> String {
        let json = try await poll { try await api.getSlot(groupId: flow.groupId(), pairingId: flow.pairingId(), slot: "challenge") }
        return try flow.receiveChallenge(challengeJson: json)
    }

    /// 既存端末での承認を待ち、グループの秘密を保存する。
    func waitForWelcome(_ flow: JoinFlow) async throws {
        let json = try await poll { try await api.getSlot(groupId: flow.groupId(), pairingId: flow.pairingId(), slot: "welcome") }
        let group = try flow.receiveWelcome(welcomeJson: json)
        try SecretStore.saveGroup(group)
    }

    private func poll(_ fetch: () async throws -> String?) async throws -> String {
        while true {
            try Task.checkCancellation()
            if let value = try await fetch() {
                return value
            }
            try await Task.sleep(for: .seconds(1))
        }
    }

    // MARK: 送受信（仕様 4.2、4.3）

    func send(text: String) async throws {
        guard let group = try SecretStore.group() else { return }
        let envelope = try sealText(
            group: group,
            deviceKey: SecretStore.deviceKey(),
            text: text,
            sensitive: false,
            nowMs: nowMs(),
            ttlMs: Config.itemTTLMs
        )
        try await api.postItem(group: group, envelopeJson: envelope)
    }

    /// 新着の合図を受ける WebSocket を開く。ペアリング前は nil。
    func openItemSignals() throws -> ItemSignalSocket? {
        guard let group = try SecretStore.group() else { return nil }
        return api.openItemSignals(group: group)
    }

    /// 新しいアイテムを取得して履歴に入れ、受け入れたものを返す。
    @discardableResult
    func sync() async throws -> [HistoryItem] {
        guard let group = try SecretStore.group() else { return [] }
        let deviceKey = try SecretStore.deviceKey()
        var accepted: [HistoryItem] = []
        while true {
            let cursor = try LocalStore.read().cursor
            let page = try await api.items(group: group, after: cursor)
            if page.isEmpty { break }
            let now = nowMs()
            let newItems = try LocalStore.update { state -> [HistoryItem] in
                var added: [HistoryItem] = []
                for item in page where item.seq > state.cursor {
                    state.cursor = item.seq
                    let outcome = try acceptItem(
                        group: group,
                        deviceKey: deviceKey,
                        stateJson: state.receiveStateJson,
                        envelopeJson: item.envelopeJson,
                        nowMs: now
                    )
                    state.receiveStateJson = outcome.stateJson
                    if let record = outcome.item {
                        added.append(HistoryItem(
                            id: record.id,
                            text: record.text,
                            fromSelf: record.fromSelf,
                            createdMs: record.createdMs,
                            expiresMs: record.expiresMs
                        ))
                    }
                }
                // 表示順は暗号文の中の作成時刻で決める（仕様 4.3）。
                state.history = Array((added + state.history).sorted { $0.createdMs > $1.createdMs }.prefix(Config.historyLimit))
                return added
            }
            accepted += newItems
        }
        // 期限を過ぎた履歴は表示しない（仕様 4.4）。
        try LocalStore.update { state in
            let now = nowMs()
            state.history.removeAll { $0.expiresMs <= now }
        }
        return accepted
    }

    func history() -> [HistoryItem] {
        (try? LocalStore.read().history) ?? []
    }
}
