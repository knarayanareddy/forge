import SwiftUI

struct PermissionsView: View {
    @Bindable var workspace: WorkspaceStore

    var body: some View {
        Form {
            Section("Workspace grant") {
                if let path = workspace.workspacePath {
                    Text(path)
                        .font(.callout.monospaced())
                    Text("Bookmark persisted locally. The daemon receives workspace_path in run_task and inserts capability_grants for loop plans.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    Text("No workspace selected.")
                        .foregroundStyle(.secondary)
                }
                Button("Change Workspace…") {
                    workspace.selectWorkspace()
                }
            }

            Section("Session") {
                LabeledContent("Session ID", value: workspace.sessionId)
            }

            Section("BYOK (Keychain)") {
                Text("Cloud API keys are stored in macOS Keychain (service AetherForge, account byok-api-key). Set AETHER_BYOK_PROVIDER=openai on the daemon to route completions through BYOK. Fail-closed on non-macOS.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding()
    }
}
