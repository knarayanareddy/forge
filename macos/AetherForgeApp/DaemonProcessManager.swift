import Foundation

final class DaemonProcessManager: @unchecked Sendable {
    static let shared = DaemonProcessManager()
    private var process: Process?
    private let lock = NSLock()

    func daemonBinaryURL() -> URL? {
        if let envPath = ProcessInfo.processInfo.environment["AETHER_DAEMON_BIN"], !envPath.isEmpty,
           FileManager.default.isExecutableFile(atPath: envPath) {
            return URL(fileURLWithPath: envPath)
        }
        let bundled = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/aether-daemon")
        if FileManager.default.isExecutableFile(atPath: bundled.path) { return bundled }
        if let exec = Bundle.main.executableURL {
            let repoRoot = exec.deletingLastPathComponent().deletingLastPathComponent()
                .deletingLastPathComponent().deletingLastPathComponent()
            for url in [
                exec.deletingLastPathComponent().appendingPathComponent("aether-daemon"),
                repoRoot.appendingPathComponent("target/debug/aether-daemon"),
                repoRoot.appendingPathComponent("target/release/aether-daemon"),
                URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("target/debug/aether-daemon"),
            ] where FileManager.default.isExecutableFile(atPath: url.path) { return url }
        }
        return nil
    }

    @discardableResult
    func ensureRunning() -> Bool {
        lock.lock(); defer { lock.unlock() }
        if let process, process.isRunning { return true }
        guard let url = daemonBinaryURL() else { return false }
        let proc = Process()
        proc.executableURL = url
        proc.environment = Self.daemonEnvironment(base: ProcessInfo.processInfo.environment)
        proc.standardOutput = FileHandle.nullDevice
        proc.standardError = FileHandle.nullDevice
        do { try proc.run(); process = proc; return true } catch { return false }
    }

    static func daemonEnvironment(base: [String: String]) -> [String: String] {
        var env = base
        let d = UserDefaults.standard
        if let registry = ModelRegistryLoader.discoverRegistryURL() { env["AETHER_MODEL_REGISTRY"] = registry.path }
        if let v = d.string(forKey: SettingsModel.Keys.primaryProfile), !v.isEmpty { env["AETHER_MODEL_PROFILE"] = v }
        if let v = d.string(forKey: SettingsModel.Keys.complexProfile), !v.isEmpty { env["AETHER_MODEL_PROFILE_COMPLEX"] = v }
        if ByokKeychain.isConfigured, let p = d.string(forKey: SettingsModel.Keys.byokProvider), !p.isEmpty {
            env["AETHER_BYOK_PROVIDER"] = p
            if let m = d.string(forKey: SettingsModel.Keys.byokModel), !m.isEmpty { env["AETHER_BYOK_MODEL"] = m }
        }
        return env
    }

    func ensureRunningAndReady() async throws {
        let client = DaemonClient.shared
        if case .success = await client.ping() { return }
        guard ensureRunning() else { throw DaemonProcessError.binaryNotFound }
        _ = await DaemonAuth.waitForToken()
        for _ in 0..<20 {
            try await Task.sleep(nanoseconds: 250_000_000)
            if case .success = await client.ping() { return }
        }
        throw DaemonProcessError.startTimeout
    }

    func shutdown() {
        lock.lock(); defer { lock.unlock() }
        guard let process, process.isRunning else { self.process = nil; return }
        process.terminate(); process.waitUntilExit(); self.process = nil
    }
}

enum DaemonProcessError: LocalizedError {
    case binaryNotFound, startTimeout
    var errorDescription: String? {
        switch self {
        case .binaryNotFound: "aether-daemon not found. Build with cargo build -p aether-daemon."
        case .startTimeout: "aether-daemon did not respond to ping within 5 seconds."
        }
    }
}
