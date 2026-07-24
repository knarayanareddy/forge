import SwiftUI

struct ChatView: View {
    @Bindable var model: AppModel
    @Bindable var workspace: WorkspaceStore

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    if model.streamedTokens.isEmpty && model.eventLog.isEmpty {
                        Text("Send a prompt to stream tokens from the daemon.")
                            .foregroundStyle(.secondary)
                    } else {
                        if !model.streamedTokens.isEmpty {
                            Text(model.streamedTokens)
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        ForEach(model.eventLog.filter { $0.type != "token" }) { event in
                            Text(event.displayLine)
                                .font(.caption.monospaced())
                                .foregroundStyle(event.type == "error" ? .red : .secondary)
                        }
                    }
                }
                .padding()
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            Divider()

            HStack {
                TextField("Prompt…", text: $model.prompt, axis: .vertical)
                    .lineLimit(1...4)
                    .disabled(model.isRunningTask)
                Button(model.isRunningTask ? "Running…" : "Send") {
                    Task {
                        await model.sendPrompt(
                            sessionId: workspace.sessionId,
                            workspacePath: workspace.workspacePath
                        )
                    }
                }
                .disabled(model.isRunningTask || model.prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .keyboardShortcut(.return, modifiers: .command)
            }
            .padding()
        }
    }
}
