import SwiftUI

struct PendingApproval: Sendable {
    let prompt: String
    let riskySteps: [String]
}

struct ApprovalPromptView: View {
    let pending: PendingApproval
    let onApprove: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label("Approval required", systemImage: "hand.raised.fill")
                .font(.headline)

            Text("The daemon blocked this plan before any tool ran. Review the risky steps below, then approve to execute the full plan with zero pre-approval side effects (PERM-02).")
                .font(.callout)
                .foregroundStyle(.secondary)

            GroupBox("Risky steps") {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(Array(pending.riskySteps.enumerated()), id: \.offset) { _, step in
                        Text(step)
                            .font(.caption.monospaced())
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(4)
            }

            Text("Prompt")
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(pending.prompt)
                .font(.callout.monospaced())
                .lineLimit(4)
                .textSelection(.enabled)

            HStack {
                Button("Cancel", role: .cancel, action: onCancel)
                Spacer()
                Button("Approve & Run", action: onApprove)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(minWidth: 420)
    }
}
