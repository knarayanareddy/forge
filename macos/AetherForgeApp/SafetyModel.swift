import SwiftUI

struct CheckpointEntry: Identifiable, Sendable {
    let id: Int64
    let createdAt: Date
}

@MainActor
@Observable
final class SafetyModel {
    var checkpoints: [CheckpointEntry] = []
    var lastUndoResult: String?
    var lastRewindResult: String?
    var isBusy = false
    var lastError: String?

    private let client = DaemonClient.shared

    func undoWrites(sessionId: String) async {
        isBusy = true
        lastError = nil

        do {
            let result = try await client.undoWrites(sessionId: sessionId)
            var summary = "Reverted \(result.revertedPaths.count) file(s)."
            if !result.notUndone.isEmpty {
                summary += " \(result.notUndone.count) not undone: \(result.notUndone.joined(separator: "; "))"
            }
            lastUndoResult = summary
        } catch {
            lastError = error.localizedDescription
        }

        isBusy = false
    }

    func createCheckpoint(sessionId: String) async {
        isBusy = true
        lastError = nil

        do {
            let checkpointId = try await client.createCheckpoint(sessionId: sessionId)
            checkpoints.append(CheckpointEntry(id: checkpointId, createdAt: Date()))
        } catch {
            lastError = error.localizedDescription
        }

        isBusy = false
    }

    func rewindCheckpoint(id: Int64) async {
        isBusy = true
        lastError = nil

        do {
            let result = try await client.rewindCheckpoint(checkpointId: id)
            var summary = "Reverted \(result.revertedPaths.count) file(s), truncated \(result.turnsTruncated) turn(s)."
            if !result.notUndone.isEmpty {
                summary += " \(result.notUndone.count) not undone: \(result.notUndone.joined(separator: "; "))"
            }
            lastRewindResult = summary
        } catch {
            lastError = error.localizedDescription
        }

        isBusy = false
    }
}
