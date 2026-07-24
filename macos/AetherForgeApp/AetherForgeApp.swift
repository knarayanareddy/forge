import SwiftUI

@main
struct AetherForgeApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}

struct ContentView: View {
    var body: some View {
        VStack(spacing: 12) {
            Text("AetherForge")
                .font(.title)
            Text("Local-first Mac agent runtime")
                .foregroundStyle(.secondary)
        }
        .padding()
        .frame(minWidth: 420, minHeight: 280)
    }
}
