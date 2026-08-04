import Foundation

struct ModelProfileEntry: Identifiable, Sendable {
    let id: String
    let backend: String
    let endpoint: String?
    let model: String?
    let provider: String?
    let contextLen: Int?
    let role: String?
    let description: String?

    var isChatProfile: Bool { role != "embed" }

    var isInferenceReady: Bool {
        backend == "ollama" || backend == "openai_compatible"
    }

    var summaryLine: String {
        switch backend {
        case "ollama":
            return model.map { "ollama · \($0)" } ?? "ollama"
        case "openai_compatible":
            return model.map { "\(provider ?? "openai") · \($0)" } ?? "openai_compatible"
        case "mlx", "gguf":
            return "\(backend) · download only"
        default:
            return backend
        }
    }
}

struct ModelRegistrySnapshot: Sendable {
    let path: String
    let version: Int
    let defaultProfile: String
    let defaultComplexProfile: String?
    let profiles: [ModelProfileEntry]
}

enum ModelRegistryLoader {
    static let defaultRelativePath = "models/registry.toml"
    static let envRegistryPath = "AETHER_MODEL_REGISTRY"

    static func discoverRegistryURL() -> URL? {
        if let override = ProcessInfo.processInfo.environment[envRegistryPath], !override.isEmpty {
            let url = URL(fileURLWithPath: override)
            if FileManager.default.fileExists(atPath: url.path) { return url }
        }

        if let repo = repoRootRegistryURL(), FileManager.default.fileExists(atPath: repo.path) {
            return repo
        }

        let cwd = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent(defaultRelativePath)
        if FileManager.default.fileExists(atPath: cwd.path) { return cwd }

        let userDefault = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".aether/registry.toml")
        if FileManager.default.fileExists(atPath: userDefault.path) { return userDefault }

        let appSupport = AppPaths.supportDirectory.appendingPathComponent("registry.toml")
        if FileManager.default.fileExists(atPath: appSupport.path) { return appSupport }

        return nil
    }

    static func load() throws -> ModelRegistrySnapshot {
        guard let url = discoverRegistryURL() else {
            throw ModelRegistryLoaderError.notFound
        }
        let raw = try String(contentsOf: url, encoding: .utf8)
        return try parse(raw, path: url.path)
    }

    static func fetchOllamaTags(endpoint: String) async throws -> [String] {
        let base = endpoint.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard let url = URL(string: base + "/api/tags") else {
            throw ModelRegistryLoaderError.invalidEndpoint(endpoint)
        }
        let (data, response) = try await URLSession.shared.data(from: url)
        guard let http = response as? HTTPURLResponse, (200 ..< 300).contains(http.statusCode) else {
            throw ModelRegistryLoaderError.ollamaUnavailable
        }
        guard
            let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
            let models = json["models"] as? [[String: Any]]
        else {
            return []
        }
        return models.compactMap { $0["name"] as? String }.sorted()
    }

    private static func repoRootRegistryURL() -> URL? {
        guard let exec = Bundle.main.executableURL else { return nil }
        let repoRoot = exec
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        return repoRoot.appendingPathComponent(defaultRelativePath)
    }

    static func parse(_ raw: String, path: String) throws -> ModelRegistrySnapshot {
        var version = 1
        var defaultProfile = ""
        var defaultComplexProfile: String?
        var profiles: [String: [String: String]] = [:]
        var currentProfile: String?

        for line in raw.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty || trimmed.hasPrefix("#") { continue }

            if trimmed.hasPrefix("[profiles.") {
                let inner = trimmed.dropFirst("[profiles.".count).dropLast()
                currentProfile = String(inner)
                profiles[currentProfile!] = [:]
                continue
            }

            guard let eq = trimmed.firstIndex(of: "=") else { continue }
            let key = trimmed[..<eq].trimmingCharacters(in: .whitespaces)
            var value = String(trimmed[trimmed.index(after: eq)...].trimmingCharacters(in: .whitespaces))
            if value.hasPrefix("\""), value.hasSuffix("\""), value.count >= 2 {
                value = String(value.dropFirst().dropLast())
            }

            if let currentProfile {
                profiles[currentProfile]?[key] = value
            } else if key == "version", let parsed = Int(value) {
                version = parsed
            } else if key == "default_profile" {
                defaultProfile = value
            } else if key == "default_complex_profile" {
                defaultComplexProfile = value
            }
        }

        guard version == 1 else {
            throw ModelRegistryLoaderError.unsupportedVersion(version)
        }
        guard !defaultProfile.isEmpty else {
            throw ModelRegistryLoaderError.parse("missing default_profile")
        }

        let entries = profiles.map { id, fields in
            ModelProfileEntry(
                id: id,
                backend: fields["backend"] ?? "unknown",
                endpoint: fields["endpoint"],
                model: fields["model"],
                provider: fields["provider"],
                contextLen: fields["context_len"].flatMap(Int.init),
                role: fields["role"],
                description: fields["description"]
            )
        }.sorted { $0.id < $1.id }

        return ModelRegistrySnapshot(
            path: path,
            version: version,
            defaultProfile: defaultProfile,
            defaultComplexProfile: defaultComplexProfile,
            profiles: entries
        )
    }
}

enum ModelRegistryLoaderError: LocalizedError {
    case notFound
    case parse(String)
    case unsupportedVersion(Int)
    case invalidEndpoint(String)
    case ollamaUnavailable

    var errorDescription: String? {
        switch self {
        case .notFound:
            "Model registry not found. Expected models/registry.toml or ~/.aether/registry.toml."
        case .parse(let detail):
            "Invalid registry TOML: \(detail)"
        case .unsupportedVersion(let v):
            "Unsupported registry version \(v) (expected 1)."
        case .invalidEndpoint(let endpoint):
            "Invalid Ollama endpoint: \(endpoint)"
        case .ollamaUnavailable:
            "Could not reach Ollama /api/tags."
        }
    }
}
