import Darwin
import Foundation

enum DaemonClientError: LocalizedError {
    case connectFailed(String)
    case sendFailed
    case receiveFailed
    case invalidEvent(String)

    var errorDescription: String? {
        switch self {
        case .connectFailed(let detail): "Could not connect to daemon: \(detail)"
        case .sendFailed: "Failed to send request to daemon"
        case .receiveFailed: "Connection closed unexpectedly"
        case .invalidEvent(let line): "Invalid daemon event: \(line)"
        }
    }
}

struct DaemonEvent: Identifiable, Sendable {
    let id = UUID()
    let type: String
    let text: String?
    let content: String?
    let message: String?
    let model: String?
    let ttftMs: Int?
    let iteration: Int?
    let tool: String?
    let output: String?
    let action: String?
    let summary: String?
    let passed: Bool?
    let detail: String?
    let revertedPaths: [String]?
    let notUndone: [String]?
    let checkpointId: Int64?
    let turnsTruncated: Int?

    var displayLine: String {
        switch type {
        case "token":
            return text ?? ""
        case "plan":
            return "[plan \(iteration ?? 0)] \(action ?? "")"
        case "tool":
            return "[tool \(iteration ?? 0)] \(tool ?? "") → \(output ?? "")"
        case "observe":
            return "[observe \(iteration ?? 0)] \(summary ?? "")"
        case "verify":
            let status = (passed ?? false) ? "PASS" : "FAIL"
            return "[verify \(iteration ?? 0)] \(status): \(detail ?? "")"
        case "done":
            return "✓ \(content ?? "done")"
        case "error":
            return "✗ \(message ?? "error")"
        case "pong":
            return "pong"
        default:
            return "[\(type)]"
        }
    }
}

final class DaemonClient: @unchecked Sendable {
    static let shared = DaemonClient()

    private let endpoint: DaemonConfig.Endpoint

    init(endpoint: DaemonConfig.Endpoint = DaemonConfig.load()) {
        self.endpoint = endpoint
    }

    var endpointDescription: String {
        "\(endpoint.host):\(endpoint.port)"
    }

    func ping(timeoutSeconds: TimeInterval = 3) async -> Result<DaemonEvent, DaemonClientError> {
        do {
            let events = try await collectEvents(
                method: "ping",
                params: authParams(),
                timeoutSeconds: timeoutSeconds
            )
            if let pong = events.first(where: { $0.type == "pong" }) {
                return .success(pong)
            }
            if let error = events.first(where: { $0.type == "error" }) {
                return .failure(.invalidEvent(error.message ?? "daemon error"))
            }
            return .failure(.invalidEvent("no pong"))
        } catch let error as DaemonClientError {
            return .failure(error)
        } catch {
            return .failure(.connectFailed(error.localizedDescription))
        }
    }

    /// Explicit grant flow after the user selects a workspace folder.
    func grantWorkspace(
        sessionId: String,
        workspacePath: String,
        timeoutSeconds: TimeInterval = 5
    ) async throws {
        var params = authParams()
        params["session_id"] = sessionId
        params["workspace_path"] = workspacePath
        let events = try await collectEvents(
            method: "grant_workspace",
            params: params,
            timeoutSeconds: timeoutSeconds
        )
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "workspace grant failed")
        }
        guard events.contains(where: { $0.type == "workspace_granted" }) else {
            throw DaemonClientError.invalidEvent("workspace grant was not acknowledged")
        }
    }

    struct UndoResult: Sendable {
        let revertedPaths: [String]
        let notUndone: [String]
    }

    func undoWrites(sessionId: String, timeoutSeconds: TimeInterval = 10) async throws -> UndoResult {
        var params = authParams()
        params["session_id"] = sessionId
        let events = try await collectEvents(
            method: "undo_writes",
            params: params,
            timeoutSeconds: timeoutSeconds
        )
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "undo failed")
        }
        guard let complete = events.first(where: { $0.type == "undo_complete" }) else {
            throw DaemonClientError.invalidEvent("undo was not acknowledged")
        }
        return UndoResult(
            revertedPaths: complete.revertedPaths ?? [],
            notUndone: complete.notUndone ?? []
        )
    }

    func createCheckpoint(sessionId: String, timeoutSeconds: TimeInterval = 10) async throws -> Int64 {
        var params = authParams()
        params["session_id"] = sessionId
        let events = try await collectEvents(
            method: "create_checkpoint",
            params: params,
            timeoutSeconds: timeoutSeconds
        )
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "checkpoint creation failed")
        }
        guard let created = events.first(where: { $0.type == "checkpoint_created" }),
              let checkpointId = created.checkpointId else {
            throw DaemonClientError.invalidEvent("checkpoint creation was not acknowledged")
        }
        return checkpointId
    }

    struct RewindResult: Sendable {
        let revertedPaths: [String]
        let notUndone: [String]
        let turnsTruncated: Int
    }

    func rewindCheckpoint(checkpointId: Int64, timeoutSeconds: TimeInterval = 10) async throws -> RewindResult {
        var params = authParams()
        params["checkpoint_id"] = checkpointId
        let events = try await collectEvents(
            method: "rewind_checkpoint",
            params: params,
            timeoutSeconds: timeoutSeconds
        )
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "rewind failed")
        }
        guard let complete = events.first(where: { $0.type == "rewind_complete" }) else {
            throw DaemonClientError.invalidEvent("rewind was not acknowledged")
        }
        return RewindResult(
            revertedPaths: complete.revertedPaths ?? [],
            notUndone: complete.notUndone ?? [],
            turnsTruncated: complete.turnsTruncated ?? 0
        )
    }

    /// Streams daemon events incrementally as JSON-lines arrive over TCP.
    func runTask(
        prompt: String,
        sessionId: String,
        workspacePath: String?,
        timeoutSeconds: TimeInterval = 120
    ) -> AsyncThrowingStream<DaemonEvent, Error> {
        var params = authParams()
        params["prompt"] = prompt
        params["session_id"] = sessionId
        if let workspacePath, !workspacePath.isEmpty {
            params["workspace_path"] = workspacePath
        }

        return stream(method: "run_task", params: params, timeoutSeconds: timeoutSeconds)
    }

    private func authParams() -> [String: Any] {
        if let token = DaemonAuth.loadToken() {
            return ["auth_token": token]
        }
        return [:]
    }

    private func stream(
        method: String,
        params: [String: Any],
        timeoutSeconds: TimeInterval
    ) -> AsyncThrowingStream<DaemonEvent, Error> {
        let payload: [String: Any] = ["method": method, "params": params]
        let requestData: Data
        do {
            requestData = try JSONSerialization.data(withJSONObject: payload)
        } catch {
            return AsyncThrowingStream { $0.finish(throwing: error) }
        }

        return AsyncThrowingStream { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    try self.streamSync(requestData: requestData, timeoutSeconds: timeoutSeconds) { event in
                        continuation.yield(event)
                        return event.type != "done" && event.type != "error" && event.type != "pong"
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    private func collectEvents(
        method: String,
        params: [String: Any],
        timeoutSeconds: TimeInterval
    ) async throws -> [DaemonEvent] {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    var events: [DaemonEvent] = []
                    try self.streamSync(
                        requestData: try JSONSerialization.data(withJSONObject: ["method": method, "params": params]),
                        timeoutSeconds: timeoutSeconds
                    ) { event in
                        events.append(event)
                        return event.type != "done" && event.type != "error" && event.type != "pong"
                    }
                    continuation.resume(returning: events)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    /// Reads TCP bytes, parses complete JSON-lines, and invokes `onEvent` for each event.
    /// Return `false` from `onEvent` to stop reading early.
    private func streamSync(
        requestData: Data,
        timeoutSeconds: TimeInterval,
        onEvent: (DaemonEvent) -> Bool
    ) throws {
        let socketFD = try connect()
        defer { close(socketFD) }

        guard var request = String(data: requestData, encoding: .utf8) else {
            throw DaemonClientError.sendFailed
        }
        request.append("\n")
        guard request.withCString({ write(socketFD, $0, strlen($0)) }) > 0 else {
            throw DaemonClientError.sendFailed
        }

        var buffer = Data()
        let deadline = Date().addingTimeInterval(timeoutSeconds)

        while Date() < deadline {
            var chunk = [UInt8](repeating: 0, count: 4096)
            let received = recv(socketFD, &chunk, chunk.count, 0)
            if received == 0 {
                break
            }
            if received < 0 {
                throw DaemonClientError.receiveFailed
            }
            buffer.append(contentsOf: chunk.prefix(received))

            while let newlineRange = buffer.firstRange(of: Data([0x0A])) {
                let lineData = buffer.subdata(in: 0..<newlineRange.lowerBound)
                buffer.removeSubrange(0..<newlineRange.upperBound)
                guard let line = String(data: lineData, encoding: .utf8)?
                    .trimmingCharacters(in: .whitespacesAndNewlines),
                      !line.isEmpty else { continue }

                let event = try parseEvent(line)
                if !onEvent(event) {
                    return
                }
            }
        }

        throw DaemonClientError.receiveFailed
    }

    private func connect() throws -> Int32 {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw DaemonClientError.connectFailed("socket() failed")
        }

        var addr = sockaddr_in()
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = endpoint.port.bigEndian
        inet_pton(AF_INET, endpoint.host, &addr.sin_addr)

        let result = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard result == 0 else {
            close(fd)
            throw DaemonClientError.connectFailed("\(endpoint.host):\(endpoint.port)")
        }
        return fd
    }

    private func parseEvent(_ line: String) throws -> DaemonEvent {
        guard let data = line.data(using: .utf8),
              let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = json["type"] as? String else {
            throw DaemonClientError.invalidEvent(line)
        }

        return DaemonEvent(
            type: type,
            text: json["text"] as? String,
            content: json["content"] as? String,
            message: json["message"] as? String,
            model: json["model"] as? String,
            ttftMs: json["ttft_ms"] as? Int,
            iteration: json["iteration"] as? Int,
            tool: json["tool"] as? String,
            output: json["output"] as? String,
            action: json["action"] as? String,
            summary: json["summary"] as? String,
            passed: json["passed"] as? Bool,
            detail: json["detail"] as? String,
            revertedPaths: json["reverted_paths"] as? [String],
            notUndone: json["not_undone"] as? [String],
            checkpointId: json["checkpoint_id"] as? Int64,
            turnsTruncated: json["turns_truncated"] as? Int
        )
    }
}
