import Foundation

/// アプリとキーボード拡張で共有する設定。
enum Config {
    /// アプリとキーボード拡張で保存領域とキーチェーンを共有するための App Group。
    static let appGroup = "group.net.dryk.enclip"

    /// 中継サーバーの URL（Info.plist の EnclipServerURL）。
    static var serverURL: URL {
        guard
            let text = Bundle.main.object(forInfoDictionaryKey: "EnclipServerURL") as? String,
            let url = URL(string: text)
        else {
            fatalError("EnclipServerURL is missing in Info.plist")
        }
        return url
    }

    /// 普通のテキストを残す期間（PoC の固定値。Windows と同じ 7 日）。
    static let itemTTLMs: UInt64 = 7 * 24 * 60 * 60 * 1000

    /// キーボードに並べる件数。
    static let keyboardItemCount = 8

    /// 履歴として保存する件数。
    static let historyLimit = 50
}

/// UNIX 時刻（ミリ秒）。
func nowMs() -> UInt64 {
    UInt64(Date().timeIntervalSince1970 * 1000)
}
