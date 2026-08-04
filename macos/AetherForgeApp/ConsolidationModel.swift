import SwiftUI

@MainActor
@Observable
final class ConsolidationModel {
    var pendingRuns: [ConsolidationRunItem] = []
    var selectedRunId: Int64?
    var previewMarkdown: String?
    var lastActionSummary: String?
    var isBusy = false
    var lastError: String?

    private let client = DaemonClient.shared

    func refreshPending() async {
        isBusy = true
        lastError = nil
        do {
            try await DaemonProcessManager.shared.ensureRunningAndReady()
            pendingRuns = try await client.listConsolidationPending()
            if let selectedRunId, !pendingRuns.contains(where: { $0.id == selectedRunId }) {
                self.selectedRunId = pendingRuns.first?.id
            } else if selectedRunId == nil {
                selectedRunId = pendingRuns.first?.id
            }
            loadPreviewForSelection()
        } catch {
            lastError = error.localizedDescription
        }
        isBusy = false
    }

    func selectRun(_ runId: Int64) {
        selectedRunId = runId
        loadPreviewForSelection()
    }

    func applySelected() async {
        guard let runId = selectedRunId else { return }
        isBusy = true
        lastError = nil
        do {
            try await DaemonProcessManager.shared.ensureRunningAndReady()
            let superseded = try await client.applyConsolidation(runId: runId)
            lastActionSummary = "Applied run #\(runId): \(superseded) node(s) superseded."
            await refreshPending()
        } catch {
            lastError = error.localizedDescription
            isBusy = false
        }
    }

    func rejectSelected() async {
        guard let runId = selectedRunId else { return }
        isBusy = true
        lastError = nil
        do {
            try await DaemonProcessManager.shared.ensureRunningAndReady()
            try await client.rejectConsolidation(runId: runId)
            lastActionSummary = "Rejected run #\(runId). Graph unchanged."
            await refreshPending()
        } catch {
            lastError = error.localizedDescription
            isBusy = false
        }
    }

    private func loadPreviewForSelection() {
        previewMarkdown = nil
        guard let runId = selectedRunId,
              let run = pendingRuns.first(where: { $0.id == runId }),
              let jsonPath = run.reviewArtifactPath else { return }
        let mdPath = jsonPath.replacingOccurrences(of: ".json", with: ".md")
        if let markdown = try? String(contentsOfFile: mdPath, encoding: .utf8) {
            previewMarkdown = markdown
        }
    }
}
