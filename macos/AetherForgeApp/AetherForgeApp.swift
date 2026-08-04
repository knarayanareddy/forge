import SwiftUI

@main
struct AetherForgeApp: App {
    @State private var workspace = WorkspaceStore()
    @State private var model = AppModel()
    @State private var safetyModel = SafetyModel()
    @State private var consolidationModel = ConsolidationModel()
    @State private var settingsModel = SettingsModel()

    var body: some Scene {
        WindowGroup {
            RootView(
                workspace: workspace,
                model: model,
                safetyModel: safetyModel,
                consolidationModel: consolidationModel,
                settingsModel: settingsModel
            )
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
    @Bindable var consolidationModel: ConsolidationModel
    @Bindable var settingsModel: SettingsModel

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

            Tab("Settings", systemImage: "gearshape") {
                SettingsView(settings: settingsModel, workspace: workspace)
            }

            Tab("Memory", systemImage: "brain.head.profile") {
                ConsolidationView(model: consolidationModel)
            }

            Tab("Subagents", systemImage: "person.2") {
                SubagentPanelView(model: model)
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
