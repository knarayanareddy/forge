import AppKit
import SwiftUI

struct OnboardingView: View {
    @Bindable var workspace: WorkspaceStore

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Welcome to AetherForge")
                .font(.title2.bold())
            Text("Select a workspace folder. A security-scoped bookmark is saved under Application Support and the path is sent to the daemon with each task.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            if let path = workspace.workspacePath {
                Label(path, systemImage: "folder.fill")
                    .font(.callout)
            }

            if let error = workspace.lastError {
                Text(error)
                    .foregroundStyle(.red)
                    .font(.callout)
            }

            Button("Choose Workspace Folder…") {
                workspace.selectWorkspace()
            }
            .buttonStyle(.borderedProminent)

            Text("Data: ~/Library/Application Support/AetherForge/")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}
