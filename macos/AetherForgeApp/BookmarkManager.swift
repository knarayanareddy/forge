import Foundation

/// Manages macOS security-scoped bookmarks for workspace folder grants (Spec v1.2.3 §4).
public final class BookmarkManager {
    public enum BookmarkError: Error {
        case creationFailed
        case resolutionFailed
        case staleBookmark
    }

    public init() {}

    /// Acquire a security-scoped bookmark from a user-selected workspace URL.
    public func createBookmark(for url: URL) throws -> Data {
        do {
            return try url.bookmarkData(
                options: [.withSecurityScope],
                includingResourceValuesForKeys: nil,
                relativeTo: nil
            )
        } catch {
            throw BookmarkError.creationFailed
        }
    }

    /// Resolve a persisted bookmark and optionally detect staleness.
    public func resolveBookmark(_ data: Data) throws -> (url: URL, isStale: Bool) {
        var isStale = false
        do {
            let url = try URL(
                resolvingBookmarkData: data,
                options: [.withSecurityScope],
                relativeTo: nil,
                bookmarkDataIsStale: &isStale
            )
            return (url, isStale)
        } catch {
            throw BookmarkError.resolutionFailed
        }
    }

    /// Activate security scope for file operations; always pair with `stopAccess`.
    @discardableResult
    public func startAccess(for url: URL) -> Bool {
        url.startAccessingSecurityScopedResource()
    }

    public func stopAccess(for url: URL, didStart: Bool) {
        if didStart {
            url.stopAccessingSecurityScopedResource()
        }
    }
}
