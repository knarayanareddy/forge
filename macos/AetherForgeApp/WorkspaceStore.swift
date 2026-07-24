import AppKit
import Foundation

enum AppPaths {
    static var supportDirectory: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let dir = base.appendingPathComponent("AetherForge", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    static var bookmarkFile: URL {
        supportDirectory.appendingPathComponent("workspace.bookmark")
    }

    static var metadataFile: URL {
        supportDirectory.appendingPathComponent("workspace.json")
    }
}

struct WorkspaceMetadata: Codable {
    var workspacePath: String
    var sessionId: String
    var bookmarkSavedAt: Date
}

@MainActor
@Observable
final class WorkspaceStore {
    private(set) var workspacePath: String?
    private(set) var sessionId: String
    private(set) var hasBookmark: Bool = false
    private(set) var lastError: String?

    private let bookmarkManager = BookmarkManager()
    private let defaults = UserDefaults.standard

    init() {
        sessionId = defaults.string(forKey: "aether.sessionId") ?? UUID().uuidString
        defaults.set(sessionId, forKey: "aether.sessionId")
        loadPersistedWorkspace()
    }

    var needsOnboarding: Bool {
        workspacePath == nil || !hasBookmark
    }

    func selectWorkspace() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Grant Workspace"
        panel.message = "Choose a folder for AetherForge agent file access."

        guard panel.runModal() == .OK, let url = panel.url else { return }

        do {
            let bookmark = try bookmarkManager.createBookmark(for: url)
            try bookmark.write(to: AppPaths.bookmarkFile, options: .atomic)
            let metadata = WorkspaceMetadata(
                workspacePath: url.path,
                sessionId: sessionId,
                bookmarkSavedAt: Date()
            )
            let data = try JSONEncoder().encode(metadata)
            try data.write(to: AppPaths.metadataFile, options: .atomic)
            workspacePath = url.path
            hasBookmark = true
            lastError = nil
        } catch {
            lastError = "Failed to save workspace bookmark: \(error.localizedDescription)"
        }
    }

    func resolvedWorkspaceURL() -> URL? {
        guard let data = try? Data(contentsOf: AppPaths.bookmarkFile) else { return nil }
        do {
            let (url, isStale) = try bookmarkManager.resolveBookmark(data)
            if isStale {
                let refreshed = try bookmarkManager.createBookmark(for: url)
                try refreshed.write(to: AppPaths.bookmarkFile, options: .atomic)
            }
            return url
        } catch {
            lastError = "Bookmark resolution failed: \(error.localizedDescription)"
            return nil
        }
    }

    func withSecurityScopedAccess<T>(_ body: (URL) throws -> T) rethrows -> T? {
        guard let url = resolvedWorkspaceURL() else { return nil }
        let started = bookmarkManager.startAccess(for: url)
        defer { bookmarkManager.stopAccess(for: url, didStart: started) }
        return try body(url)
    }

    private func loadPersistedWorkspace() {
        if let data = try? Data(contentsOf: AppPaths.metadataFile),
           let metadata = try? JSONDecoder().decode(WorkspaceMetadata.self, from: data) {
            workspacePath = metadata.workspacePath
            sessionId = metadata.sessionId
            defaults.set(sessionId, forKey: "aether.sessionId")
        }
        hasBookmark = FileManager.default.fileExists(atPath: AppPaths.bookmarkFile.path)
    }
}