import SwiftUI

@main
struct AetherForgeApp: App {
    @State private var workspace = WorkspaceStore()
    @State private var model = AppModel()
    @State private var safetyModel = SafetyModel()

    var body: some Scene {
        WindowGroup {
            RootView(workspace: workspace, model: model, safetyModel: safetyModel)
                .frame(minWidth: 640, minHeight: 480)
                .task {
                    await model.ensureDaemonAndConnect()
                }
                .onDisappear {
                    DaemonProcessManager.shared.shutdown()
                }
        }
    }
}

struct RootView: View {
    @Bindable var workspace: WorkspaceStore
    @Bindable var model: AppModel
    @Bindable var safetyModel: SafetyModel

    var body: some View {
        TabView {
            Tab("Chat", systemImage: "bubble.left.and.bubble.right") {
                if workspace.needsOnboarding {
                    OnboardingView(workspace: workspace)
                } else {
                    ChatView(model: model, workspace: workspace)
                }
            }

            Tab("Workspace", systemImage: "folder") {
                OnboardingView(workspace: workspace)
            }

            Tab("Permissions", systemImage: "lock.shield") {
                PermissionsView(workspace: workspace)
            }

            Tab("Activity", systemImage: "waveform.path.ecg") {
                ActivityView(model: model)
            }

            Tab("Safety", systemImage: "arrow.uturn.backward.circle") {
                SafetyView(model: safetyModel, sessionId: workspace.sessionId)
            }
        }
    }
}
