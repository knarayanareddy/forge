import AetherFFI
import Foundation

enum DaemonConfig {
    struct Endpoint: Sendable {
        let host: String
        let port: UInt16
        let contract: String
    }

    static func load() -> Endpoint {
        let port = aether_daemon_default_port()
        let ipcPtr = aether_ffi_daemon_ipc()
        defer { if let ipcPtr { aether_free_string(ipcPtr) } }

        guard let ipcPtr else {
            return Endpoint(host: "127.0.0.1", port: port, contract: "tcp-json-lines:127.0.0.1:\(port)")
        }
        let contract = String(cString: ipcPtr)
        let addr = contract.replacingOccurrences(of: "tcp-json-lines:", with: "")
        let parts = addr.split(separator: ":", maxSplits: 1).map(String.init)
        let host = parts.first ?? "127.0.0.1"
        let parsedPort = parts.count > 1 ? UInt16(parts[1]) ?? port : port
        return Endpoint(host: host, port: parsedPort, contract: contract)
    }
}
