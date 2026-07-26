import SwiftUI

@MainActor
@Observable
final class AppModel {
    enum ConnectionStatus: String {
        case unknown = "Unknown"
        case connected = "Connected"
        case disconnected = "Disconnected"
        case busy = "Busy"
    }

    var connectionStatus: ConnectionStatus = .unknown
    var lastPingAt: Date?
    var lastResponseSummary: String = "No tasks yet"
    var streamedTokens: String = ""
    var eventLog: [DaemonEvent] = []
    var prompt: String = ""
    var isRunningTask = false
    var lastError: String?

    private let client = DaemonClient.shared

    var daemonEndpoint: String {
        client.endpointDescription
    }

    func ensureDaemonAndConnect() async {
        await refreshConnection()
    }

    func refreshConnection() async {
        guard !isRunningTask else { return }
        do {
            try await DaemonProcessManager.shared.ensureRunningAndReady()
            connectionStatus = .connected
            lastPingAt = Date()
            lastError = nil
        } catch {
            connectionStatus = .disconnected
            lastError = error.localizedDescription
        }
    }

    func sendPrompt(sessionId: String, workspacePath: String?) async {
        let trimmed = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        isRunningTask = true
        connectionStatus = .busy
        streamedTokens = ""
        eventLog = []
        lastError = nil

        do {
            try await DaemonProcessManager.shared.ensureRunningAndReady()
            if let workspacePath, !workspacePath.isEmpty {
                try await client.grantWorkspace(
                    sessionId: sessionId,
                    workspacePath: workspacePath
                )
            }
        } catch {
            lastError = error.localizedDescription
            lastResponseSummary = error.localizedDescription
            connectionStatus = .disconnected
            isRunningTask = false
            return
        }

        let stream = client.runTask(
            prompt: trimmed,
            sessionId: sessionId,
            workspacePath: workspacePath
        )

        do {
            for try await event in stream {
                eventLog.append(event)
                if event.type == "token", let text = event.text {
                    streamedTokens.append(text)
                }
                if event.type == "done" {
                    lastResponseSummary = event.content ?? "Task completed"
                    if let model = event.model {
                        lastResponseSummary += " (\(model))"
                    }
                }
                if event.type == "error" {
                    lastError = event.message
                    lastResponseSummary = event.message ?? "Task failed"
                }
            }
            connectionStatus = .connected
            lastPingAt = Date()
        } catch {
            lastError = error.localizedDescription
            lastResponseSummary = error.localizedDescription
            connectionStatus = .disconnected
        }

        isRunningTask = false
        prompt = ""
    }
}
