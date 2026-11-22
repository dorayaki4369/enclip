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
                await refresh()
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
        await refresh()
    }
}
