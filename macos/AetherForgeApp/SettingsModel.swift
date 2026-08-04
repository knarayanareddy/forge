import SwiftUI

@MainActor
@Observable
final class SettingsModel {
    enum Keys {
        static let primaryProfile = "aether.modelProfile"
        static let complexProfile = "aether.modelProfileComplex"
        static let byokProvider = "aether.byokProvider"
        static let byokModel = "aether.byokModel"
    }

    var registry: ModelRegistrySnapshot?
    var selectedPrimaryProfile: String = ""
    var selectedComplexProfile: String = ""
    var ollamaInstalledModels: [String] = []
    var ollamaEndpoint: String = "http://localhost:11434"
    var byokKeyDraft: String = ""
    var byokProvider: String = "openai"
    var byokModel: String = "gpt-4o-mini"
    var byokKeyConfigured = false
    var isBusy = false
    var lastActionSummary: String?
    var lastError: String?

    private let defaults = UserDefaults.standard
    private let client = DaemonClient.shared

    var chatProfiles: [ModelProfileEntry] {
        registry?.profiles.filter(\.isChatProfile) ?? envFallbackProfiles
    }

    var daemonEnvironmentSummary: String {
        let env = DaemonProcessManager.daemonEnvironment(base: ProcessInfo.processInfo.environment)
        let keys = [
            "AETHER_MODEL_REGISTRY",
            "AETHER_MODEL_PROFILE",
            "AETHER_MODEL_PROFILE_COMPLEX",
            "AETHER_BYOK_PROVIDER",
            "AETHER_BYOK_MODEL",
            "AETHER_OLLAMA_ENDPOINT",
            "AETHER_CHAT_MODEL",
        ]
        return keys.compactMap { key in
            guard let value = env[key], !value.isEmpty else { return nil }
            return "\(key)=\(value)"
        }.joined(separator: "\n")
    }

    func refresh() async {
        isBusy = true
        lastError = nil
        loadSavedPreferences()
        byokKeyConfigured = ByokKeychain.isConfigured

        do {
            registry = try ModelRegistryLoader.load()
            if selectedPrimaryProfile.isEmpty {
                selectedPrimaryProfile = defaults.string(forKey: Keys.primaryProfile)
                    ?? registry?.defaultProfile
                    ?? ""
            }
            if selectedComplexProfile.isEmpty {
                selectedComplexProfile = defaults.string(forKey: Keys.complexProfile)
                    ?? registry?.defaultComplexProfile
                    ?? selectedPrimaryProfile
            }
            if let endpoint = registry?.profiles.first(where: { $0.backend == "ollama" })?.endpoint {
                ollamaEndpoint = endpoint
            }
        } catch {
            registry = nil
            if selectedPrimaryProfile.isEmpty {
                selectedPrimaryProfile = envFallbackProfiles.first?.id ?? ""
                selectedComplexProfile = envFallbackProfiles.last?.id ?? selectedPrimaryProfile
            }
        }

        _ = try? await client.fetchModelConfig()
        ollamaInstalledModels = (try? await ModelRegistryLoader.fetchOllamaTags(endpoint: ollamaEndpoint)) ?? []
        isBusy = false
    }

    func applyProfileSelection() {
        guard !selectedPrimaryProfile.isEmpty else { return }
        defaults.set(selectedPrimaryProfile, forKey: Keys.primaryProfile)
        defaults.set(selectedComplexProfile, forKey: Keys.complexProfile)
        lastActionSummary =
            "Saved profile \(selectedPrimaryProfile) (complex: \(selectedComplexProfile)). Restart the daemon to apply."
        lastError = nil
    }

    func saveByokKey() {
        isBusy = true
        lastError = nil
        Task {
            defer { isBusy = false }
            do {
                if try await client.storeBYOKKey(byokKeyDraft) {
                    lastActionSummary = "API key stored via daemon IPC."
                } else {
                    try ByokKeychain.storeKey(byokKeyDraft)
                    lastActionSummary =
                        "API key saved to Keychain (no daemon IPC yet — direct Keychain write, matching Rust loader)."
                }
                byokKeyDraft = ""
                byokKeyConfigured = ByokKeychain.isConfigured
            } catch {
                lastError = error.localizedDescription
            }
        }
    }

    func clearByokKey() {
        isBusy = true
        lastError = nil
        Task {
            defer { isBusy = false }
            do {
                _ = try? await client.deleteBYOKKey()
                try ByokKeychain.deleteKey()
                byokKeyConfigured = false
                byokKeyDraft = ""
                lastActionSummary = "BYOK API key removed from Keychain."
            } catch {
                lastError = error.localizedDescription
            }
        }
    }

    func saveByokRouting() {
        defaults.set(byokProvider, forKey: Keys.byokProvider)
        defaults.set(byokModel, forKey: Keys.byokModel)
        lastActionSummary =
            "Saved BYOK routing (\(byokProvider) / \(byokModel)). Restart the daemon to apply AETHER_BYOK_PROVIDER."
        lastError = nil
    }

    private func loadSavedPreferences() {
        byokProvider = defaults.string(forKey: Keys.byokProvider)
            ?? ProcessInfo.processInfo.environment["AETHER_BYOK_PROVIDER"]
            ?? "openai"
        byokModel = defaults.string(forKey: Keys.byokModel)
            ?? ProcessInfo.processInfo.environment["AETHER_BYOK_MODEL"]
            ?? "gpt-4o-mini"
        if let primary = defaults.string(forKey: Keys.primaryProfile) {
            selectedPrimaryProfile = primary
        }
        if let complex = defaults.string(forKey: Keys.complexProfile) {
            selectedComplexProfile = complex
        }
    }

    private var envFallbackProfiles: [ModelProfileEntry] {
        let env = ProcessInfo.processInfo.environment
        let chat = env["AETHER_CHAT_MODEL"] ?? "qwen2.5:3b"
        let complex = env["AETHER_CHAT_MODEL_COMPLEX"] ?? chat
        let endpoint = env["AETHER_OLLAMA_ENDPOINT"] ?? "http://localhost:11434"
        return [
            ModelProfileEntry(
                id: "env-primary",
                backend: "ollama",
                endpoint: endpoint,
                model: chat,
                provider: nil,
                contextLen: nil,
                role: nil,
                description: "From AETHER_CHAT_MODEL"
            ),
            ModelProfileEntry(
                id: "env-complex",
                backend: "ollama",
                endpoint: endpoint,
                model: complex,
                provider: nil,
                contextLen: nil,
                role: nil,
                description: "From AETHER_CHAT_MODEL_COMPLEX"
            ),
        ]
    }
}
