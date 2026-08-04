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

            Section("Model & BYOK") {
                Text("Configure model profiles and cloud API keys in the Settings tab.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding()
    }
}
