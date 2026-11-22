import SwiftUI
import UIKit

/// enclip のキーボード。開いたときに新着を取得し、最新の数件をタップで入力する。
final class KeyboardViewController: UIInputViewController {
    private let model = KeyboardModel()

    override func viewDidLoad() {
        super.viewDidLoad()
        let root = KeyboardView(
            model: model,
            insert: { [weak self] text in self?.textDocumentProxy.insertText(text) },
            deleteBackward: { [weak self] in self?.textDocumentProxy.deleteBackward() },
            nextKeyboard: { [weak self] in self?.advanceToNextInputMode() }
        )
        let host = UIHostingController(rootView: root)
        host.view.translatesAutoresizingMaskIntoConstraints = false
        host.view.backgroundColor = .clear
        addChild(host)
        view.addSubview(host.view)
        NSLayoutConstraint.activate([
            host.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            host.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            host.view.topAnchor.constraint(equalTo: view.topAnchor),
            host.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            host.view.heightAnchor.constraint(equalToConstant: 260),
        ])
        host.didMove(toParent: self)
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        model.showsNextKeyboard = needsInputModeSwitchKey
        model.load(hasFullAccess: hasFullAccess)
    }
}

@MainActor
@Observable
final class KeyboardModel {
    private let service = EnclipService()
    var items: [HistoryItem] = []
    var status: String?
    var showsNextKeyboard = true

    func load(hasFullAccess: Bool) {
        items = Array(service.history().filter { !$0.fromSelf }.prefix(Config.keyboardItemCount))
        guard hasFullAccess else {
            status = "設定 > 一般 > キーボード > enclip で「フルアクセスを許可」をオンにしてください"
            return
        }
        guard service.isPaired else {
            status = "enclip アプリで Windows とペアリングしてください"
            return
        }
        status = "更新中…"
        Task {
            do {
                try await service.sync()
                status = nil
            } catch {
                status = "更新できませんでした"
            }
            items = Array(service.history().filter { !$0.fromSelf }.prefix(Config.keyboardItemCount))
        }
    }
}

struct KeyboardView: View {
    let model: KeyboardModel
    let insert: (String) -> Void
    let deleteBackward: () -> Void
    let nextKeyboard: () -> Void

    var body: some View {
        VStack(spacing: 6) {
            if let status = model.status {
                Text(status).font(.caption).foregroundStyle(.secondary).padding(.horizontal, 8)
            }
            ScrollView {
                VStack(spacing: 6) {
                    ForEach(model.items) { item in
                        Button {
                            insert(item.text)
                        } label: {
                            Text(item.text)
                                .lineLimit(2)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(10)
                                .background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 8))
                        }
                        .buttonStyle(.plain)
                    }
                    if model.items.isEmpty && model.status == nil {
                        Text("Windows でコピーした内容がここに出ます").font(.caption).foregroundStyle(.secondary)
                    }
                }
                .padding(.horizontal, 8)
            }
            HStack {
                if model.showsNextKeyboard {
                    Button(action: nextKeyboard) { Image(systemName: "globe") }
                        .accessibilityLabel("次のキーボード")
                }
                Spacer()
                Button(action: deleteBackward) { Image(systemName: "delete.left") }
                    .accessibilityLabel("削除")
            }
            .font(.title3)
            .padding(.horizontal, 12)
            .padding(.bottom, 6)
        }
        .padding(.top, 6)
    }
}
