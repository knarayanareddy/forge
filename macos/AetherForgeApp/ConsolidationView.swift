import SwiftUI

private struct RunRow: View {
    let run: ConsolidationRunItem
    let isSelected: Bool
    let onSelect: () -> Void

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text("Run #\(run.id)").font(.headline)
                Text("Started \(run.startedAt)").font(.caption).foregroundStyle(.secondary)
                Text("\(run.inputNodeCount) nodes, \(run.dedupeCount) dedupes").font(.callout)
            }
            Spacer()
            if isSelected {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.tint)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(perform: onSelect)
    }
}

struct ConsolidationView: View {
    @Bindable var model: ConsolidationModel

    var body: some View {
        Form {
            Section {
                Text("Review pending memory consolidations before they are applied to the knowledge graph (CONS-01).")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Pending review") {
                if model.pendingRuns.isEmpty {
                    Text("No runs awaiting review.").foregroundStyle(.secondary)
                } else {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(model.pendingRuns, id: \ConsolidationRunItem.id) { (run: ConsolidationRunItem) in
                            RunRow(run: run, isSelected: model.selectedRunId == run.id) {
                                model.selectRun(run.id)
                            }
                        }
                    }
                }

                HStack {
                    Button("Refresh") { Task { await model.refreshPending() } }
                        .disabled(model.isBusy)
                    Spacer()
                    Button("Reject") { Task { await model.rejectSelected() } }
                        .disabled(model.isBusy || model.selectedRunId == nil)
                    Button("Apply") { Task { await model.applySelected() } }
                        .disabled(model.isBusy || model.selectedRunId == nil)
                }
            }

            if let preview = model.previewMarkdown {
                Section("Preview") {
                    ScrollView {
                        Text(preview)
                            .font(.caption.monospaced())
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(minHeight: 160, maxHeight: 320)
                }
            }

            if let summary = model.lastActionSummary {
                Section("Last action") { Text(summary).font(.callout) }
            }

            if let error = model.lastError {
                Section { Text(error).foregroundStyle(.red).font(.caption) }
            }
        }
        .formStyle(.grouped)
        .padding()
        .task { await model.refreshPending() }
    }
}
