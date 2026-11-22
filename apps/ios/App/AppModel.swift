import Observation
import SwiftUI
import UIKit

/// アプリの画面の状態。
@MainActor
@Observable
final class AppModel {
    enum PairingStage: Equatable {
        case notPaired
        case sending
        /// 確認コードを表示し、Windows での承認を待っている。
        case waitingApproval(code: String)
        case paired
    }

    private let service = EnclipService()
    private var pairingTask: Task<Void, Never>?
    private var liveTask: Task<Void, Never>?

    var stage: PairingStage
    var history: [HistoryItem] = []
    var message: String?

    init() {
        stage = service.isPaired ? .paired : .notPaired
        history = service.history()
    }

    /// QR コードの文字列で参加を始める。
    func join(qrText: String) {
        pairingTask?.cancel()
        stage = .sending
        message = nil
        pairingTask = Task {
            do {
                let flow = try await service.startJoin(qrText: qrText)
                let code = try await service.waitForCode(flow)
                stage = .waitingApproval(code: code)
                try await service.waitForWelcome(flow)
                stage = .paired
                message = "ペアリングしました"
                startLive()
            } catch is CancellationError {
                stage = .notPaired
            } catch {
                stage = .notPaired
                message = "ペアリングに失敗しました: \(error.localizedDescription)"
            }
        }
    }

    func cancelPairing() {
        pairingTask?.cancel()
        pairingTask = nil
        stage = service.isPaired ? .paired : .notPaired
    }

    func send(text: String) async {
        guard !text.isEmpty else { return }
        do {
            try await service.send(text: text)
            message = "送信しました"
            await refresh()
        } catch {
            message = "送信に失敗しました: \(error.localizedDescription)"
        }
    }

    /// アプリが前面にある間、新着の合図を待って自動で取得する。
    func startLive() {
        guard liveTask == nil, service.isPaired else { return }
        liveTask = Task { await runLive() }
    }

    func stopLive() {
        liveTask?.cancel()
        liveTask = nil
    }

    private static let pingInterval: Duration = .seconds(25)
    private static let maxRetryDelay: Duration = .seconds(30)
    /// この時間より長く続いた接続の後は、再接続の待ち時間を最初に戻す。
    private static let stableConnection: Duration = .seconds(30)

    private func runLive() async {
        let clock = ContinuousClock()
        var retryDelay: Duration = .seconds(1)
        while !Task.isCancelled {
            guard let socket = try? service.openItemSignals() else { return }
            let connectedAt = clock.now
            // 接続する前に届いていたものを取る。以降は合図のたびに取る。
            await refresh()
            await withTaskCancellationHandler {
                await withThrowingTaskGroup(of: Void.self) { group in
                    group.addTask {
                        while true {
                            try await socket.nextSignal()
                            await self.refresh()
                        }
                    }
                    group.addTask {
                        while true {
                            try await Task.sleep(for: Self.pingInterval)
                            try await socket.ping()
                        }
                    }
                    // どちらかが投げたら接続が切れたとみなす。受信待ちは取り消しでは止まらないので、
                    // 接続を閉じて止めてからグループを抜ける。
                    _ = try? await group.next()
                    socket.close()
                    group.cancelAll()
                }
            } onCancel: {
                socket.close()
            }
            socket.close()
            if clock.now - connectedAt > Self.stableConnection {
                retryDelay = .seconds(1)
            }
            try? await Task.sleep(for: retryDelay)
            retryDelay = min(retryDelay * 2, Self.maxRetryDelay)
        }
    }

    func refresh() async {
        guard service.isPaired else { return }
        do {
            try await service.sync()
        } catch {
            message = "受信に失敗しました: \(error.localizedDescription)"
        }
        history = service.history()
    }

    /// パスワード欄に貼るため、iOS のクリップボードへ入れる。
    func copy(_ item: HistoryItem) {
        UIPasteboard.general.string = item.text
        message = "コピーしました"
    }

    /// 開発用（DEBUG ビルドだけ）。シミュレーターでカメラやボタン操作なしに確かめるための起動引数。
    /// - `-enclipJoinQR <QR の文字列>`: 参加を始める
    /// - `-enclipSendText <テキスト>`: テキストを送る
    func startFromLaunchArguments() async {
        #if DEBUG
        let arguments = ProcessInfo.processInfo.arguments
        func value(after flag: String) -> String? {
            guard let index = arguments.firstIndex(of: flag), arguments.indices.contains(index + 1) else { return nil }
            return arguments[index + 1]
        }
        if let qr = value(after: "-enclipJoinQR"), stage == .notPaired {
            join(qrText: qr)
        }
        if let text = value(after: "-enclipSendText"), stage == .paired {
            await send(text: text)
        }
        #endif
    }
}
