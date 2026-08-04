import SwiftUI

struct SubagentDelegation: Identifiable, Sendable {
    let id = UUID()
    let iteration: Int
    let summary: String
    let succeeded: Bool
}

enum SubagentEventParser {
    static func delegations(from events: [DaemonEvent]) -> [SubagentDelegation] {
        events.compactMap { event in
            guard event.type == "tool", event.tool == "subagent_task", let iteration = event.iteration else { return nil }
            let output = event.output ?? ""
            let succeeded = !output.lowercased().contains("error")
            return SubagentDelegation(iteration: iteration, summary: output.isEmpty ? "(empty)" : output, succeeded: succeeded)
        }
    }
}

struct SubagentPanelView: View {
    @Bindable var model: AppModel
    private var delegations: [SubagentDelegation] { SubagentEventParser.delegations(from: model.eventLog) }

    var body: some View {
        Form {
            Section {
                Text("Subagents return one distilled summary per delegation (SUB-01).")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("Delegations this session") {
                if delegations.isEmpty {
                    Text("No subagent delegations yet.").foregroundStyle(.secondary)
                } else {
                    ForEach(delegations) { d in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text("Iteration \(d.iteration)").font(.headline)
                                Spacer()
                                Label(d.succeeded ? "OK" : "Failed", systemImage: d.succeeded ? "checkmark.circle" : "xmark.circle")
                                    .font(.caption).foregroundStyle(d.succeeded ? .green : .red)
                            }
                            Text(d.summary).font(.caption.monospaced()).lineLimit(8).textSelection(.enabled)
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .padding()
    }
}
