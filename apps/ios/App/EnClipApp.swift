import SwiftUI

@main
struct EnClipApp: App {
    @State private var model = AppModel()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView(model: model)
                .task { await model.startFromLaunchArguments() }
                .onChange(of: scenePhase, initial: true) { _, phase in
                    // 前面にある間だけ接続し、新着を自動で取る。接続時に取り残しも取る。
                    if phase == .active {
                        model.startLive()
                    } else {
                        model.stopLive()
                    }
                }
        }
    }
}
