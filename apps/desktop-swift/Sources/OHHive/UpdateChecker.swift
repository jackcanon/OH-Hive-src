import Foundation

/// "How do folks know they need to update?" (Jack, 2026-09-13) -- until now the answer was
/// nothing: no auto-updater exists for either desktop shell (ADR-010 decision 13 planned a Tauri
/// updater, never built; ADR-018's Swift shell doesn't mention updates at all), and the "release
/// notes on login" feature (#178, `ReleaseNotesView.swift`) is a changelog only -- it never
/// compares versions or says "you're behind."
///
/// Deliberately independent of the Hive hub: this hits GitHub's public releases API directly, not
/// `hub.rs`/Supabase, both because a client update check shouldn't depend on being paired or
/// signed in at all, and because `hub.rs` is mid-refactor for ADR-025 (the `Hub` trait/`LocalHub`
/// work) in this same session -- adding unrelated surface there right now would be asking for a
/// collision. If a real coordinator-backed version gate is ever built (see
/// `docs/CONTROL-PLANE-MIGRATION-PLAN-2026-09-13.md`), this can move onto it; nothing here assumes
/// GitHub specifically beyond the one URL below.
///
/// Fails silently on any error (offline, GitHub down, unexpected JSON) -- same "nice to have,
/// never block opening the app" posture `ContentView`'s release-notes check already uses.
enum UpdateChecker {
    /// The public mirror release.yml publishes every tagged build to -- see that workflow's
    /// "publish to public mirror" step. Public repo, no auth needed.
    private static let latestReleaseURL = URL(
        string: "https://api.github.com/repos/jackcanon/ohhive-releases/releases/latest"
    )!

    struct AvailableUpdate: Equatable {
        let version: String
        let url: URL
    }

    private struct GitHubRelease: Decodable {
        let tagName: String
        let htmlUrl: String

        enum CodingKeys: String, CodingKey {
            case tagName = "tag_name"
            case htmlUrl = "html_url"
        }
    }

    /// Returns the newer version + a link to it, or `nil` if the current version is already
    /// current (or the check couldn't complete for any reason). `currentVersion` should be
    /// `store.about.appVersion` (the `ohhive-ffi` crate's own `CARGO_PKG_VERSION`, which is what
    /// gets tagged as `vX.Y.Z` for a release -- see `release.yml`'s `${GITHUB_REF_NAME#v}`).
    static func check(currentVersion: String) async -> AvailableUpdate? {
        guard
            let (data, response) = try? await URLSession.shared.data(from: latestReleaseURL),
            let http = response as? HTTPURLResponse,
            http.statusCode == 200,
            let release = try? JSONDecoder().decode(GitHubRelease.self, from: data)
        else {
            return nil
        }
        let latest = release.tagName.hasPrefix("v") ? String(release.tagName.dropFirst()) : release.tagName
        guard isNewer(latest, than: currentVersion), let url = URL(string: release.htmlUrl) else {
            return nil
        }
        return AvailableUpdate(version: latest, url: url)
    }

    /// Plain dot-separated numeric comparison (`"0.3.0"` > `"0.2.10"`), missing/non-numeric
    /// components treated as 0 -- good enough for this project's tagging scheme (`vX.Y.Z`, no
    /// pre-release suffixes in use); a malformed tag just fails the "is newer" check rather than
    /// crashing, which is the right failure mode for a nice-to-have banner.
    private static func isNewer(_ a: String, than b: String) -> Bool {
        let ap = a.split(separator: ".").map { Int($0) ?? 0 }
        let bp = b.split(separator: ".").map { Int($0) ?? 0 }
        for i in 0..<max(ap.count, bp.count) {
            let x = i < ap.count ? ap[i] : 0
            let y = i < bp.count ? bp[i] : 0
            if x != y { return x > y }
        }
        return false
    }
}
