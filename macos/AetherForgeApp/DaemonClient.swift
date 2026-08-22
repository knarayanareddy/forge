import CryptoKit
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

struct ConsolidationRunItem: Identifiable, Sendable {
    let id: Int64
    let status: String
    let inputNodeCount: Int64
    let dedupeCount: Int64
    let contradictionCount: Int64
    let reviewArtifactPath: String?
    let reviewMarkdown: String?
    let reviewHash: String?
    let startedAt: String
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
    let riskySteps: [String]?
    let runId: Int64?
    let nodesSuperseded: Int?
    let consolidationRuns: [ConsolidationRunItem]?
    let serverProof: String?
    let approvalId: String?
    let approvalPlan: String?
    let approvalDigest: String?
    let approvalExpiresAt: UInt64?

    var displayLine: String {
        switch type {
        case "token": return text ?? ""
        case "plan": return "[plan \(iteration ?? 0)] \(action ?? "")"
        case "tool": return "[tool \(iteration ?? 0)] \(tool ?? "") → \(output ?? "")"
        case "observe": return "[observe \(iteration ?? 0)] \(summary ?? "")"
        case "verify":
            let status = (passed ?? false) ? "PASS" : "FAIL"
            return "[verify \(iteration ?? 0)] \(status): \(detail ?? "")"
        case "done": return "✓ \(content ?? "done")"
        case "error": return "✗ \(message ?? "error")"
        case "pong": return "pong"
        case "pending_approval":
            return "⚠ approval required: \(riskySteps?.joined(separator: "; ") ?? "risky steps")"
        case "consolidation_list":
            return "consolidation runs: \(consolidationRuns?.count ?? 0)"
        case "consolidation_applied":
            return "✓ applied run #\(runId ?? 0) (\(nodesSuperseded ?? 0) nodes superseded)"
        case "consolidation_rejected":
            return "✗ rejected run #\(runId ?? 0)"
        default: return "[\(type)]"
        }
    }
}

final class DaemonClient: @unchecked Sendable {
    static let shared = DaemonClient()
    private let endpoint: DaemonConfig.Endpoint

    init(endpoint: DaemonConfig.Endpoint = DaemonConfig.load()) {
        self.endpoint = endpoint
    }

    var endpointDescription: String { "\(endpoint.host):\(endpoint.port)" }

    func ping(timeoutSeconds: TimeInterval = 3) async -> Result<DaemonEvent, DaemonClientError> {
        do {
            guard let token = DaemonAuth.loadToken() else {
                return .failure(.invalidEvent("daemon identity token unavailable"))
            }
            let nonce = Self.randomNonce()
            let events = try await collectEvents(
                method: "ping",
                params: ["client_nonce": nonce],
                timeoutSeconds: timeoutSeconds
            )
            if let error = events.first(where: { $0.type == "error" }) {
                return .failure(.invalidEvent(error.message ?? "daemon error"))
            }
            guard let pong = events.first(where: { $0.type == "pong" }),
                  let receivedProof = pong.serverProof else {
                return .failure(.invalidEvent("daemon did not prove possession of the identity token"))
            }
            let expectedProof = Self.daemonProof(token: token, nonce: nonce)
            guard Self.constantTimeEqual(receivedProof, expectedProof) else {
                return .failure(.invalidEvent("daemon identity proof mismatch"))
            }
            return .success(pong)
        } catch let error as DaemonClientError {
            return .failure(error)
        } catch {
            return .failure(.connectFailed(error.localizedDescription))
        }
    }

    func grantWorkspace(sessionId: String, workspacePath: String, timeoutSeconds: TimeInterval = 5) async throws {
        var params = authParams()
        params["session_id"] = sessionId
        params["workspace_path"] = workspacePath
        let events = try await collectEvents(method: "grant_workspace", params: params, timeoutSeconds: timeoutSeconds)
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "workspace grant failed")
        }
        guard events.contains(where: { $0.type == "workspace_granted" }) else {
            throw DaemonClientError.invalidEvent("workspace grant was not acknowledged")
        }
    }

    struct UndoResult: Sendable { let revertedPaths: [String]; let notUndone: [String] }

    func undoWrites(sessionId: String, timeoutSeconds: TimeInterval = 10) async throws -> UndoResult {
        var params = authParams()
        params["session_id"] = sessionId
        let events = try await collectEvents(method: "undo_writes", params: params, timeoutSeconds: timeoutSeconds)
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "undo failed")
        }
        guard let complete = events.first(where: { $0.type == "undo_complete" }) else {
            throw DaemonClientError.invalidEvent("undo was not acknowledged")
        }
        return UndoResult(revertedPaths: complete.revertedPaths ?? [], notUndone: complete.notUndone ?? [])
    }

    func createCheckpoint(sessionId: String, timeoutSeconds: TimeInterval = 10) async throws -> Int64 {
        var params = authParams()
        params["session_id"] = sessionId
        let events = try await collectEvents(method: "create_checkpoint", params: params, timeoutSeconds: timeoutSeconds)
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
        let events = try await collectEvents(method: "rewind_checkpoint", params: params, timeoutSeconds: timeoutSeconds)
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

    func runTask(
        prompt: String,
        sessionId: String,
        workspacePath: String?,
        executionMode: String,
        approvalId: String? = nil,
        timeoutSeconds: TimeInterval = 120
    ) -> AsyncThrowingStream<DaemonEvent, Error> {
        var params = authParams()
        params["session_id"] = sessionId
        params["execution_mode"] = executionMode
        if let approvalId {
            params["approval_id"] = approvalId
        } else {
            params["prompt"] = prompt
        }
        if let workspacePath, !workspacePath.isEmpty { params["workspace_path"] = workspacePath }
        return stream(method: "run_task", params: params, timeoutSeconds: timeoutSeconds)
    }

    func listConsolidationPending(timeoutSeconds: TimeInterval = 10) async throws -> [ConsolidationRunItem] {
        let events = try await collectEvents(
            method: "list_consolidation_pending", params: authParams(), timeoutSeconds: timeoutSeconds)
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "list failed")
        }
        guard let list = events.first(where: { $0.type == "consolidation_list" }) else {
            throw DaemonClientError.invalidEvent("consolidation list was not returned")
        }
        return list.consolidationRuns ?? []
    }

    func applyConsolidation(runId: Int64, timeoutSeconds: TimeInterval = 10) async throws -> Int {
        var params = authParams()
        params["run_id"] = runId
        let events = try await collectEvents(method: "apply_consolidation", params: params, timeoutSeconds: timeoutSeconds)
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "apply failed")
        }
        guard let applied = events.first(where: { $0.type == "consolidation_applied" }) else {
            throw DaemonClientError.invalidEvent("consolidation apply was not acknowledged")
        }
        return applied.nodesSuperseded ?? 0
    }

    func rejectConsolidation(runId: Int64, timeoutSeconds: TimeInterval = 10) async throws {
        var params = authParams()
        params["run_id"] = runId
        let events = try await collectEvents(method: "reject_consolidation", params: params, timeoutSeconds: timeoutSeconds)
        if let error = events.first(where: { $0.type == "error" }) {
            throw DaemonClientError.invalidEvent(error.message ?? "reject failed")
        }
        guard events.contains(where: { $0.type == "consolidation_rejected" }) else {
            throw DaemonClientError.invalidEvent("consolidation reject was not acknowledged")
        }
    }

    func fetchModelConfig(timeoutSeconds: TimeInterval = 5) async throws -> Bool {
        let events = try await collectEvents(method: "get_model_config", params: authParams(), timeoutSeconds: timeoutSeconds)
        if let error = events.first(where: { $0.type == "error" }) {
            let message = error.message ?? ""
            if message.contains("Unknown method") { return false }
            throw DaemonClientError.invalidEvent(message)
        }
        return events.contains(where: { $0.type == "model_config" })
    }

    func storeBYOKKey(_ apiKey: String, timeoutSeconds: TimeInterval = 5) async throws -> Bool {
        var params = authParams()
        params["api_key"] = apiKey
        let events = try await collectEvents(method: "store_byok_key", params: params, timeoutSeconds: timeoutSeconds)
        if let error = events.first(where: { $0.type == "error" }) {
            let message = error.message ?? ""
            if message.contains("Unknown method") { return false }
            throw DaemonClientError.invalidEvent(message)
        }
        return events.contains(where: { $0.type == "byok_stored" })
    }

    func deleteBYOKKey(timeoutSeconds: TimeInterval = 5) async throws -> Bool {
        let events = try await collectEvents(method: "delete_byok_key", params: authParams(), timeoutSeconds: timeoutSeconds)
        if let error = events.first(where: { $0.type == "error" }) {
            let message = error.message ?? ""
            if message.contains("Unknown method") { return false }
            throw DaemonClientError.invalidEvent(message)
        }
        return events.contains(where: { $0.type == "byok_deleted" })
    }

    private func authParams() -> [String: Any] {
        if let token = DaemonAuth.loadToken() { return ["auth_token": token] }
        return [:]
    }

    private func stream(method: String, params: [String: Any], timeoutSeconds: TimeInterval) -> AsyncThrowingStream<DaemonEvent, Error> {
        let payload: [String: Any] = ["method": method, "params": params]
        let requestData: Data
        do { requestData = try JSONSerialization.data(withJSONObject: payload) }
        catch { return AsyncThrowingStream { $0.finish(throwing: error) } }

        return AsyncThrowingStream { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    try self.streamSync(requestData: requestData, timeoutSeconds: timeoutSeconds) { event in
                        continuation.yield(event)
                        return !Self.isTerminalEvent(event.type)
                    }
                    continuation.finish()
                } catch { continuation.finish(throwing: error) }
            }
        }
    }

    private func collectEvents(method: String, params: [String: Any], timeoutSeconds: TimeInterval) async throws -> [DaemonEvent] {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    var events: [DaemonEvent] = []
                    try self.streamSync(
                        requestData: try JSONSerialization.data(withJSONObject: ["method": method, "params": params]),
                        timeoutSeconds: timeoutSeconds
                    ) { event in
                        events.append(event)
                        return !Self.isTerminalEvent(event.type)
                    }
                    continuation.resume(returning: events)
                } catch { continuation.resume(throwing: error) }
            }
        }
    }

    private func streamSync(requestData: Data, timeoutSeconds: TimeInterval, onEvent: (DaemonEvent) -> Bool) throws {
        let socketFD = try connect(timeoutSeconds: timeoutSeconds)
        defer { close(socketFD) }

        guard requestData.count <= 1_048_576 else { throw DaemonClientError.sendFailed }
        var payload = requestData
        payload.append(0x0A)
        let sentAll = payload.withUnsafeBytes { rawBuffer -> Bool in
            guard let base = rawBuffer.baseAddress else { return false }
            var sent = 0
            while sent < rawBuffer.count {
                let count = Darwin.send(socketFD, base.advanced(by: sent), rawBuffer.count - sent, 0)
                if count <= 0 { return false }
                sent += count
            }
            return true
        }
        guard sentAll else { throw DaemonClientError.sendFailed }

        var buffer = Data()
        var sawEvent = false
        while true {
            var chunk = [UInt8](repeating: 0, count: 4096)
            let received = recv(socketFD, &chunk, chunk.count, 0)
            if received == 0 {
                if sawEvent && buffer.isEmpty { return }
                throw DaemonClientError.receiveFailed
            }
            if received < 0 { throw DaemonClientError.receiveFailed }
            buffer.append(contentsOf: chunk.prefix(received))
            guard buffer.count <= 1_048_576 else {
                throw DaemonClientError.invalidEvent("daemon event exceeded 1 MiB")
            }
            while let newlineRange = buffer.firstRange(of: Data([0x0A])) {
                let lineData = buffer.subdata(in: 0..<newlineRange.lowerBound)
                buffer.removeSubrange(0..<newlineRange.upperBound)
                guard let line = String(data: lineData, encoding: .utf8)?
                    .trimmingCharacters(in: .whitespacesAndNewlines), !line.isEmpty else { continue }
                let event = try parseEvent(line)
                sawEvent = true
                if !onEvent(event) { return }
            }
        }
    }

    private func connect(timeoutSeconds: TimeInterval) throws -> Int32 {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw DaemonClientError.connectFailed("socket() failed") }

        var timeout = timeval(
            tv_sec: Int(timeoutSeconds.rounded(.down)),
            tv_usec: Int32((timeoutSeconds.truncatingRemainder(dividingBy: 1)) * 1_000_000)
        )
        let timeoutSize = socklen_t(MemoryLayout<timeval>.size)
        guard setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, timeoutSize) == 0,
              setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, timeoutSize) == 0 else {
            close(fd)
            throw DaemonClientError.connectFailed("could not configure socket timeout")
        }
        var noSigPipe: Int32 = 1
        _ = setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe, socklen_t(MemoryLayout<Int32>.size))

        var addr = sockaddr_in()
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = endpoint.port.bigEndian
        guard inet_pton(AF_INET, endpoint.host, &addr.sin_addr) == 1 else {
            close(fd)
            throw DaemonClientError.connectFailed("invalid IPv4 endpoint \(endpoint.host)")
        }
        let result = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard result == 0 else { close(fd); throw DaemonClientError.connectFailed("\(endpoint.host):\(endpoint.port)") }
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
            turnsTruncated: json["turns_truncated"] as? Int,
            riskySteps: json["risky_steps"] as? [String],
            runId: json["run_id"] as? Int64,
            nodesSuperseded: json["nodes_superseded"] as? Int,
            consolidationRuns: Self.parseConsolidationRuns(json["runs"]),
            serverProof: json["server_proof"] as? String,
            approvalId: json["approval_id"] as? String,
            approvalPlan: json["approval_plan"] as? String,
            approvalDigest: json["approval_digest"] as? String,
            approvalExpiresAt: (json["approval_expires_at"] as? NSNumber)?.uint64Value
        )
    }

    private static func isTerminalEvent(_ type: String) -> Bool {
        switch type {
        case "done", "error", "pong", "pending_approval",
             "workspace_granted", "undo_complete", "checkpoint_created", "rewind_complete",
             "automation_registered", "automation_tick", "automation_run_complete",
             "consolidation_list", "consolidation_applied", "consolidation_rejected",
             "model_config", "byok_stored", "byok_deleted":
            return true
        default:
            return false
        }
    }

    private static func randomNonce() -> String {
        var generator = SystemRandomNumberGenerator()
        return (0..<32)
            .map { _ in String(format: "%02x", UInt8.random(in: .min ... .max, using: &generator)) }
            .joined()
    }

    private static func daemonProof(token: String, nonce: String) -> String {
        let key = SymmetricKey(data: Data(token.utf8))
        var message = Data("aether-daemon-proof-v2\0".utf8)
        message.append(Data(nonce.utf8))
        return HMAC<SHA256>.authenticationCode(for: message, using: key)
            .map { String(format: "%02x", $0) }
            .joined()
    }

    private static func constantTimeEqual(_ lhs: String, _ rhs: String) -> Bool {
        let left = Array(lhs.utf8)
        let right = Array(rhs.utf8)
        guard left.count == right.count else { return false }
        return zip(left, right).reduce(UInt8(0)) { result, pair in
            result | (pair.0 ^ pair.1)
        } == 0
    }

    private static func parseConsolidationRuns(_ value: Any?) -> [ConsolidationRunItem]? {
        guard let runs = value as? [[String: Any]] else { return nil }
        return runs.compactMap { row in
            guard let runId = row["run_id"] as? Int64 ?? (row["run_id"] as? Int).map(Int64.init) else { return nil }
            return ConsolidationRunItem(
                id: runId,
                status: row["status"] as? String ?? "",
                inputNodeCount: row["input_node_count"] as? Int64 ?? (row["input_node_count"] as? Int).map(Int64.init) ?? 0,
                dedupeCount: row["dedupe_count"] as? Int64 ?? (row["dedupe_count"] as? Int).map(Int64.init) ?? 0,
                contradictionCount: row["contradiction_count"] as? Int64 ?? (row["contradiction_count"] as? Int).map(Int64.init) ?? 0,
                reviewArtifactPath: row["review_artifact_path"] as? String,
                reviewMarkdown: row["review_markdown"] as? String,
                reviewHash: row["review_hash"] as? String,
                startedAt: row["started_at"] as? String ?? ""
            )
        }
    }
}
