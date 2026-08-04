import SwiftUI

private struct ProfileRow: View {
    let profile: ModelProfileEntry
    let isPrimary: Bool
    let isComplex: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(profile.id).font(.headline)
                Spacer()
                if isPrimary {
                    Label("Primary", systemImage: "1.circle.fill").font(.caption2)
                }
                if isComplex {
                    Label("Complex", systemImage: "2.circle.fill").font(.caption2)
                }
            }
            Text(profile.summaryLine).font(.callout).foregroundStyle(.secondary)
            if let description = profile.description {
                Text(description).font(.caption).foregroundStyle(.secondary)
            }
            HStack(spacing: 8) {
                Text(profile.backend).font(.caption2.monospaced())
                    .padding(.horizontal, 6).padding(.vertical, 2)
                    .background(.quaternary, in: Capsule())
                if profile.isInferenceReady {
                    Label("Ready", systemImage: "checkmark.circle").font(.caption2).foregroundStyle(.green)
                } else {
                    Label("Deferred", systemImage: "clock").font(.caption2).foregroundStyle(.orange)
                }
            }
        }
    }
}

struct SettingsView: View {
    @Bindable var settings: SettingsModel

    var body: some View {
        Form {
            Section {
                Text("Choose model profiles from the registry TOML and configure BYOK keys in Keychain. Changes apply after daemon restart.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Model profiles") {
                if settings.chatProfiles.isEmpty {
                    Text("No chat profiles loaded.").foregroundStyle(.secondary)
                } else {
                    Picker("Primary", selection: $settings.selectedPrimaryProfile) {
                        ForEach(settings.chatProfiles) { profile in
                            Text(profile.id).tag(profile.id)
                        }
                    }
                    .disabled(settings.isBusy)

                    Picker("Complex routing", selection: $settings.selectedComplexProfile) {
                        ForEach(settings.chatProfiles) { profile in
                            Text(profile.id).tag(profile.id)
                        }
                    }
                    .disabled(settings.isBusy)

                    Button("Apply selection") { settings.applyProfileSelection() }
                        .disabled(settings.isBusy || settings.selectedPrimaryProfile.isEmpty)
                }

                if let registry = settings.registry {
                    LabeledContent("Registry", value: registry.path)
                        .font(.caption.monospaced())
                } else {
                    Text("Registry unavailable — using AETHER_CHAT_MODEL env fallback.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            Section("Catalog") {
                if settings.chatProfiles.isEmpty {
                    Text("No profiles to display.").foregroundStyle(.secondary)
                } else {
                    ForEach(settings.chatProfiles) { profile in
                        ProfileRow(
                            profile: profile,
                            isPrimary: profile.id == settings.selectedPrimaryProfile,
                            isComplex: profile.id == settings.selectedComplexProfile
                        )
                    }
                }
            }

            Section("Ollama installed") {
                if settings.ollamaInstalledModels.isEmpty {
                    Text("No models reported at \(settings.ollamaEndpoint).")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(settings.ollamaInstalledModels, id: \.self) { name in
                        Text(name).font(.callout.monospaced())
                    }
                }
                Button("Refresh") { Task { await settings.refresh() } }
                    .disabled(settings.isBusy)
            }

            Section("BYOK (Keychain)") {
                LabeledContent("Key status") {
                    Label(
                        settings.byokKeyConfigured ? "Configured" : "Not set",
                        systemImage: settings.byokKeyConfigured ? "key.fill" : "key"
                    )
                    .foregroundStyle(settings.byokKeyConfigured ? .green : .secondary)
                }

                SecureField("API key", text: $settings.byokKeyDraft)
                    .disabled(settings.isBusy)

                TextField("Provider", text: $settings.byokProvider)
                    .disabled(settings.isBusy)
                TextField("Model", text: $settings.byokModel)
                    .disabled(settings.isBusy)

                HStack {
                    Button("Save key") { settings.saveByokKey() }
                        .disabled(settings.isBusy || settings.byokKeyDraft.isEmpty)
                    Button("Clear key") { settings.clearByokKey() }
                        .disabled(settings.isBusy || !settings.byokKeyConfigured)
                    Spacer()
                    Button("Save routing") { settings.saveByokRouting() }
                        .disabled(settings.isBusy)
                }

                Text("Keys are stored locally (service AetherForge, account byok-api-key). There is no daemon IPC for BYOK yet — the app writes Keychain directly, matching the Rust daemon loader. Set provider/model above; restart the daemon. Fail-closed on non-macOS.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Daemon environment") {
                Text(settings.daemonEnvironmentSummary.isEmpty ? "(defaults)" : settings.daemonEnvironmentSummary)
                    .font(.caption.monospaced())
                    .textSelection(.enabled)
                Text("The app passes these variables when it spawns aether-daemon. Use Activity → Ping after changes.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if let summary = settings.lastActionSummary {
                Section("Last action") { Text(summary).font(.callout) }
            }

            if let error = settings.lastError {
                Section { Text(error).foregroundStyle(.red).font(.caption) }
            }
        }
        .formStyle(.grouped)
        .padding()
        .task { await settings.refresh() }
    }
}
