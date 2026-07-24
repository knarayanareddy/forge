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
            let events = try await send(method: "ping", params: [:], timeoutSeconds: timeoutSeconds)
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

    func runTask(
        prompt: String,
        sessionId: String,
        workspacePath: String?,
        timeoutSeconds: TimeInterval = 120
    ) -> AsyncThrowingStream<DaemonEvent, Error> {
        var params: [String: Any] = [
            "prompt": prompt,
            "session_id": sessionId
        ]
        if let workspacePath, !workspacePath.isEmpty {
            params["workspace_path"] = workspacePath
        }
        let requestData: Data
        do {
            requestData = try JSONSerialization.data(withJSONObject: ["method": "run_task", "params": params])
        } catch {
            return AsyncThrowingStream { $0.finish(throwing: error) }
        }

        return AsyncThrowingStream { continuation in
            Task {
                do {
                    let events = try await self.send(requestData: requestData, timeoutSeconds: timeoutSeconds)
                    for event in events {
                        continuation.yield(event)
                        if event.type == "done" || event.type == "error" {
                            break
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    private func send(
        requestData: Data,
        timeoutSeconds: TimeInterval
    ) async throws -> [DaemonEvent] {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    let events = try self.sendSync(
                        requestData: requestData,
                        timeoutSeconds: timeoutSeconds
                    )
                    continuation.resume(returning: events)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private func send(
        method: String,
        params: [String: Any],
        timeoutSeconds: TimeInterval
    ) async throws -> [DaemonEvent] {
        let payload: [String: Any] = ["method": method, "params": params]
        let requestData = try JSONSerialization.data(withJSONObject: payload)
        return try await send(requestData: requestData, timeoutSeconds: timeoutSeconds)
    }

    private func sendSync(
        requestData: Data,
        timeoutSeconds: TimeInterval
    ) throws -> [DaemonEvent] {
        let socketFD = try connect()
        defer { close(socketFD) }

        guard var request = String(data: requestData, encoding: .utf8) else {
            throw DaemonClientError.sendFailed
        }
        request.append("\n")
        guard request.withCString({ write(socketFD, $0, strlen($0)) }) > 0 else {
            throw DaemonClientError.sendFailed
        }

        var events: [DaemonEvent] = []
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
                guard let line = String(data: lineData, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
                      !line.isEmpty else { continue }
                let event = try parseEvent(line)
                events.append(event)
                if event.type == "done" || event.type == "error" || event.type == "pong" {
                    return events
                }
            }
        }

        if events.isEmpty {
            throw DaemonClientError.receiveFailed
        }
        return events
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
            detail: json["detail"] as? String
        )
    }
}
