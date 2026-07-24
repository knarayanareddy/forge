import SwiftUI

struct ActivityView: View {
    @Bindable var model: AppModel

    var body: some View {
        Form {
            Section("Daemon") {
                LabeledContent("Endpoint", value: model.daemonEndpoint)
                LabeledContent("Status") {
                    statusBadge
                }
                if let ping = model.lastPingAt {
                    LabeledContent("Last ping", value: ping.formatted(date: .abbreviated, time: .standard))
                }
            }

            Section("Last task") {
                Text(model.lastResponseSummary)
                    .font(.callout)
                if let error = model.lastError {
                    Text(error)
                        .foregroundStyle(.red)
                        .font(.caption)
                }
            }

            Section("Launch") {
                Text("Start the daemon: cargo run -p aether-daemon")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("Data: ~/Library/Application Support/AetherForge/")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                Button("Ping daemon") {
                    Task { await model.refreshConnection() }
                }
                .disabled(model.isRunningTask)
            }
        }
        .formStyle(.grouped)
        .padding()
    }

    @ViewBuilder
    private var statusBadge: some View {
        switch model.connectionStatus {
        case .connected:
            Label("Connected", systemImage: "circle.fill")
                .foregroundStyle(.green)
        case .disconnected:
            Label("Disconnected", systemImage: "circle.fill")
                .foregroundStyle(.red)
        case .busy:
            Label("Busy", systemImage: "circle.fill")
                .foregroundStyle(.orange)
        case .unknown:
            Label("Unknown", systemImage: "circle.fill")
                .foregroundStyle(.secondary)
        }
    }
}
