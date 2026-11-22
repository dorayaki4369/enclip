import Foundation

/// 受信した履歴と受信状態。アプリとキーボード拡張で同じファイルを読み書きする。
struct LocalState: Codable {
    var cursor: UInt64 = 0
    /// enclip-core の受信状態（受け入れた ID と観測した最新時刻）。
    var receiveStateJson: String?
    var history: [HistoryItem] = []
}

struct HistoryItem: Codable, Identifiable, Equatable {
    let id: String
    let text: String
    let fromSelf: Bool
    let createdMs: UInt64
    let expiresMs: UInt64
}

/// App Group の保存領域にある状態ファイル。アプリとキーボード拡張が同時に書かないよう、ファイル協調を使う。
enum LocalStore {
    private static var fileURL: URL {
        guard let container = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: Config.appGroup) else {
            fatalError("App Group \(Config.appGroup) is not configured")
        }
        return container.appendingPathComponent("state.json")
    }

    /// 状態を読む。ファイルがまだなければ空の状態を返し、読めなければエラーにする。
    /// 読めないときに空から始めると、受け入れた ID の記録が消えて再送を受け入れてしまうため（仕様 4.3）。
    static func read() throws -> LocalState {
        var result: Result<LocalState, Error> = .success(LocalState())
        coordinate(writing: false) { url in
            result = Result { try load(url) }
        }
        return try result.get()
    }

    private static func load(_ url: URL) throws -> LocalState {
        guard FileManager.default.fileExists(atPath: url.path) else { return LocalState() }
        return try JSONDecoder().decode(LocalState.self, from: Data(contentsOf: url))
    }

    /// 読み込み、変更し、書き戻すまでを 1 回の協調で行う。
    @discardableResult
    static func update<T>(_ change: (inout LocalState) throws -> T) throws -> T {
        var result: Result<T, Error> = .failure(CocoaError(.fileWriteUnknown))
        coordinate(writing: true) { url in
            result = Result {
                var state = try load(url)
                let value = try change(&state)
                let data = try JSONEncoder().encode(state)
                try data.write(to: url, options: [.atomic, .completeFileProtection])
                return value
            }
        }
        return try result.get()
    }

    private static func coordinate(writing: Bool, _ body: (URL) -> Void) {
        let coordinator = NSFileCoordinator()
        var error: NSError?
        if writing {
            coordinator.coordinate(writingItemAt: fileURL, options: .forMerging, error: &error, byAccessor: body)
        } else {
            coordinator.coordinate(readingItemAt: fileURL, options: [], error: &error, byAccessor: body)
        }
    }
}
