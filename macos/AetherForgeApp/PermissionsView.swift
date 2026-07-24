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

            Section("Future (Phase 5)") {
                Text("Keychain BYOK and deny-default Seatbelt profiles are deferred to Phase 5.")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }
        }
        .formStyle(.grouped)
        .padding()
    }
}
