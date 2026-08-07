import Foundation

#if canImport(Sparkle)
import Sparkle

@MainActor
final class SparkleUpdateController: ObservableObject {
    let updaterController: SPUStandardUpdaterController
    @Published private(set) var feedConfigured: Bool
    @Published private(set) var lastCheckSummary: String?

    init() {
        let feedURL = Bundle.main.object(forInfoDictionaryKey: "SUFeedURL") as? String
        let publicKey = Bundle.main.object(forInfoDictionaryKey: "SUPublicEDKey") as? String
        let configured = Self.isConfiguredFeed(feedURL) && Self.isConfiguredPublicKey(publicKey)
        updaterController = SPUStandardUpdaterController(
            startingUpdater: configured,
            updaterDelegate: nil,
            userDriverDelegate: nil
        )
        feedConfigured = configured
    }

    func checkForUpdates() {
        guard feedConfigured else {
            lastCheckSummary = "Sparkle feed not configured — set SUFeedURL and SUPublicEDKey after EdDSA keygen (see docs/SPARKLE.md)."
            return
        }
        lastCheckSummary = "Checking for updates…"
        updaterController.checkForUpdates(nil)
        lastCheckSummary = "Update check dispatched."
    }

    private static func isConfiguredFeed(_ url: String?) -> Bool {
        guard let url, !url.isEmpty else { return false }
        return !url.contains("__") && url.hasPrefix("https://")
    }

    private static func isConfiguredPublicKey(_ key: String?) -> Bool {
        guard let key, !key.isEmpty else { return false }
        return !key.contains("PASTE") && !key.contains("__")
    }
}
#else
@MainActor
final class SparkleUpdateController: ObservableObject {
    @Published private(set) var feedConfigured = false
    @Published private(set) var lastCheckSummary: String?
    func checkForUpdates() { lastCheckSummary = "Sparkle not linked on this platform." }
}
#endif
