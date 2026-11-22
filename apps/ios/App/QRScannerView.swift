import SwiftUI
import VisionKit

/// カメラで QR コードを読み取る（VisionKit の DataScannerViewController）。
struct QRScannerView: UIViewControllerRepresentable {
    let onScan: (String) -> Void

    func makeUIViewController(context: Context) -> UIViewController {
        guard DataScannerViewController.isSupported, DataScannerViewController.isAvailable else {
            let fallback = UIViewController()
            let label = UILabel()
            label.text = "この端末ではカメラで読み取れません。QR コードの文字列を貼り付けてください。"
            label.numberOfLines = 0
            label.textAlignment = .center
            label.frame = fallback.view.bounds.insetBy(dx: 24, dy: 24)
            label.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            fallback.view.backgroundColor = .systemBackground
            fallback.view.addSubview(label)
            return fallback
        }
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator
        try? scanner.startScanning()
        return scanner
    }

    func updateUIViewController(_ controller: UIViewController, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(onScan: onScan)
    }

    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        private let onScan: (String) -> Void
        private var done = false

        init(onScan: @escaping (String) -> Void) {
            self.onScan = onScan
        }

        func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) {
            guard !done else { return }
            for item in addedItems {
                if case let .barcode(barcode) = item, let text = barcode.payloadStringValue, text.hasPrefix("ENCLIP1:") {
                    done = true
                    dataScanner.stopScanning()
                    onScan(text)
                    return
                }
            }
        }
    }
}
