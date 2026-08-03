import SwiftUI

struct SafetyView: View {
    @Bindable var model: SafetyModel
    let sessionId: String

    var body: some View {
        Form {
            Section {
                Text("Undo reverts files the agent wrote in this session. Checkpoints let you mark a point and return to it later, including the session transcript.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Undo") {
                Button("Undo Last Writes") {
                    Task { await model.undoWrites(sessionId: sessionId) }
                }
                .disabled(model.isBusy)

                if let lastUndoResult = model.lastUndoResult {
                    Text(lastUndoResult)
                        .font(.callout)
                }
                if let error = model.lastError {
                    Text(error)
                        .foregroundStyle(.red)
                        .font(.caption)
                }
            }

            Section("Checkpoints") {
                Button("Create Checkpoint") {
                    Task { await model.createCheckpoint(sessionId: sessionId) }
                }
                .disabled(model.isBusy)

                ForEach(model.checkpoints) { checkpoint in
                    HStack {
                        Text("#\(checkpoint.id) — \(checkpoint.createdAt.formatted(date: .abbreviated, time: .standard))")
                        Spacer()
                        Button("Rewind") {
                            Task { await model.rewindCheckpoint(id: checkpoint.id) }
                        }
                        .disabled(model.isBusy)
                    }
                }

                if let lastRewindResult = model.lastRewindResult {
                    Text(lastRewindResult)
                        .font(.callout)
                }
                if let error = model.lastError {
                    Text(error)
                        .foregroundStyle(.red)
                        .font(.caption)
                }
            }
        }
        .formStyle(.grouped)
        .padding()
    }
}
