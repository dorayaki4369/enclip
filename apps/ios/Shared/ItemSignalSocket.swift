import Foundation

/// 新着の合図を受ける WebSocket（apps/server の `/ws`）。
///
/// サーバーはアイテムを受け取るたびに全端末へ知らせる。中身は使わず、合図のたびに連番で取り直す。
/// 接続ごとに専用の URLSession を持ち、`close()` で一緒に片付ける。
final class ItemSignalSocket: Sendable {
    private let session: URLSession
    private let task: URLSessionWebSocketTask

    init(request: URLRequest) {
        let configuration = URLSessionConfiguration.ephemeral
        // 合図は数分来ないこともある。生存確認は ping で行うので、待ち時間では切らない。
        configuration.timeoutIntervalForRequest = 24 * 60 * 60
        session = URLSession(configuration: configuration)
        task = session.webSocketTask(with: request)
        task.resume()
    }

    /// 次の合図まで待つ。接続が切れたら投げる。
    func nextSignal() async throws {
        _ = try await task.receive()
    }

    /// 接続が生きているかを確かめる。切れていたら投げる。
    func ping() async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            task.sendPing { error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume()
                }
            }
        }
    }

    func close() {
        task.cancel(with: .goingAway, reason: nil)
        session.invalidateAndCancel()
    }
}
