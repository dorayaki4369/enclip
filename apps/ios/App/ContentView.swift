import SwiftUI

struct ContentView: View {
    @Bindable var model: AppModel
    @State private var showScanner = false
    @State private var manualQR = ""

    var body: some View {
        NavigationStack {
            List {
                if let message = model.message {
                    Section { Text(message).foregroundStyle(.secondary) }
                }
                switch model.stage {
                case .notPaired:
                    pairingSection
                case .sending:
                    Section("ペアリング") {
                        ProgressView("Windows に参加を申請しています…")
                        Button("やめる", role: .cancel) { model.cancelPairing() }
                    }
                case let .waitingApproval(code):
                    Section("確認コード") {
                        Text(code)
                            .font(.system(size: 40, weight: .semibold, design: .monospaced))
                            .accessibilityIdentifier("pairing-code")
                        Text("Windows に同じコードが出ていることを確かめて、Windows で承認してください。")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                        Button("やめる", role: .cancel) { model.cancelPairing() }
                    }
                case .paired:
                    sendSection
                    historySection
                }
            }
            .navigationTitle("enclip")
            .refreshable { await model.refresh() }
            .sheet(isPresented: $showScanner) {
                QRScannerView { text in
                    showScanner = false
                    model.join(qrText: text)
                }
                .ignoresSafeArea()
            }
        }
    }

    private var pairingSection: some View {
        Section("Windows とペアリング") {
            Text("Windows の enclip で「端末を追加」を押し、表示された QR コードを読み取ってください。")
                .font(.footnote)
            Button("QR コードを読み取る") { showScanner = true }
            DisclosureGroup("QR コードの文字列を貼り付ける") {
                TextField("ENCLIP1:…", text: $manualQR)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                Button("参加する") { model.join(qrText: manualQR) }
                    .disabled(manualQR.isEmpty)
            }
        }
    }

    private var sendSection: some View {
        Section("送る") {
            // UIPasteControl に当たる、許可の確認なしに貼り付けられるボタン。
            PasteButton(payloadType: String.self) { strings in
                guard let text = strings.first else { return }
                Task { await model.send(text: text) }
            }
            .labelStyle(.titleAndIcon)
        }
    }

    private var historySection: some View {
        Section {
            if model.history.isEmpty {
                Text("まだ何も届いていません").foregroundStyle(.secondary)
            }
            ForEach(model.history) { item in
                Button {
                    model.copy(item)
                } label: {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(item.text).lineLimit(3).foregroundStyle(.primary)
                        Text(caption(for: item)).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
        } header: {
            Text("履歴")
        } footer: {
            Text("タップするとコピーします。パスワード欄にはこの方法で貼ってください。")
        }
    }

    private func caption(for item: HistoryItem) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(item.createdMs) / 1000)
        let source = item.fromSelf ? "この iPhone から" : "Windows から"
        return "\(source) ・ \(date.formatted(date: .abbreviated, time: .shortened))"
    }
}
