import { Nav } from "@/components/RequireMember";

// No dedicated downloads page existed before this (2026-09-13) -- the only prior pointer was a
// raw GitHub Releases link buried on /pair, framed around adding a new machine rather than
// updating an existing one. Fetches the public mirror's latest release directly (no auth needed,
// it's a public repo) so links always point at real, current assets instead of a hand-maintained
// version number going stale the next time something ships.
export const revalidate = 300;

type ReleaseAsset = { name: string; browser_download_url: string; size: number };
type Release = { tag_name: string; html_url: string; assets: ReleaseAsset[] };

const RELEASES_LATEST_API = "https://api.github.com/repos/jackcanon/ohhive-releases/releases/latest";
const RELEASES_LATEST_PAGE = "https://github.com/jackcanon/ohhive-releases/releases/latest";

async function latestRelease(): Promise<Release | null> {
  try {
    const res = await fetch(RELEASES_LATEST_API, {
      headers: { Accept: "application/vnd.github+json" },
      next: { revalidate },
    });
    if (!res.ok) return null;
    return (await res.json()) as Release;
  } catch {
    return null;
  }
}

function find(assets: ReleaseAsset[], pattern: RegExp): ReleaseAsset | undefined {
  return assets.find((a) => pattern.test(a.name));
}

function mb(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(0)} MB`;
}

export default async function DownloadPage() {
  const release = await latestRelease();
  const version = release?.tag_name?.replace(/^v/, "");
  const assets = release?.assets ?? [];

  // Exact names `.github/workflows/release.yml` produces -- see its "collect dmg"/"package" steps.
  const macSwift = find(assets, /^Hive-Swift-.*macos-aarch64\.dmg$/);
  const macTauri = find(assets, /^Hive-\d.*macos-aarch64\.dmg$/);
  const windows = find(assets, /windows-msvc\.zip$/);
  const linuxX64 = find(assets, /x86_64-unknown-linux-musl\.tar\.gz$/);
  const linuxArm = find(assets, /aarch64-unknown-linux-musl\.tar\.gz$/);
  // Added in v0.4.1. Before it, Linux got the CLI and nothing else -- the Tauri .deb/.AppImage
  // were built in CI as throwaway test artifacts and never attached to a release, so a Linux user
  // had no way to discover a UI existed at all. Both are offered rather than one: .deb for
  // Debian/Ubuntu package management, .AppImage for everything else and for no-install trial.
  const linuxDeb = find(assets, /^Hive-\d.*linux-x86_64\.deb$/);
  const linuxAppImage = find(assets, /^Hive-\d.*linux-x86_64\.AppImage$/);

  const wrap = { maxWidth: 640, margin: "48px auto", padding: "0 24px", lineHeight: 1.5 } as const;
  const card = {
    border: "1px solid var(--border)",
    borderRadius: 8,
    padding: 16,
    marginBottom: 16,
    background: "var(--surface)",
  } as const;
  const btn = {
    display: "inline-block",
    padding: "8px 14px",
    marginTop: 8,
    marginRight: 8,
    textDecoration: "none",
    border: "1px solid var(--border)",
    borderRadius: 6,
    color: "inherit",
  } as const;
  const fallback = (
    <a href={RELEASES_LATEST_PAGE} target="_blank" rel="noreferrer" style={btn}>
      Get the latest release on GitHub
    </a>
  );

  return (
    <>
      <Nav />
      <main style={wrap}>
        <h1>Download Hive</h1>
        {version ? (
          <p style={{ color: "var(--muted-strong)" }}>
            Current version: <strong>v{version}</strong>
          </p>
        ) : (
          <p style={{ color: "var(--danger)" }}>
            Couldn&apos;t reach GitHub to list the latest release just now — use the link at the
            bottom of this page instead.
          </p>
        )}

        <div style={card}>
          <p style={{ margin: "0 0 6px", fontWeight: 600 }}>Mac app (Apple Silicon)</p>
          <p style={{ margin: "0 0 8px", color: "var(--muted-strong)" }}>
            The native Hive app — setup wizard, node/server management, chat, and more. This is
            the one most people want.
          </p>
          {/* Said plainly rather than left to be discovered by downloading a dmg that will not
              open: every desktop build we ship is Apple Silicon only. An Intel Mac is not
              unsupported, it just gets the CLI below, which is the whole node either way. */}
          <p style={{ margin: "0 0 8px", color: "var(--muted)", fontSize: 13 }}>
            Apple Silicon only. On an Intel Mac, use the command line below — it is the same node,
            without the window.
          </p>
          {macSwift ? (
            <a href={macSwift.browser_download_url} style={btn}>
              Download Hive.app ({mb(macSwift.size)})
            </a>
          ) : (
            fallback
          )}
        </div>

        <div style={card}>
          <p style={{ margin: "0 0 6px", fontWeight: 600 }}>Linux app (x86_64)</p>
          <p style={{ margin: "0 0 8px", color: "var(--muted-strong)" }}>
            The same desktop app, packaged for Linux. Take the <code>.deb</code> on Debian or
            Ubuntu so your package manager tracks it; take the <code>.AppImage</code> anywhere
            else, or to try it without installing anything —{" "}
            <code>chmod +x</code> it and run it.
          </p>
          {linuxDeb || linuxAppImage ? (
            <>
              {linuxDeb && (
                <a href={linuxDeb.browser_download_url} style={btn}>
                  .deb ({mb(linuxDeb.size)})
                </a>
              )}
              {linuxAppImage && (
                <a href={linuxAppImage.browser_download_url} style={btn}>
                  .AppImage ({mb(linuxAppImage.size)})
                </a>
              )}
            </>
          ) : (
            <>
              <p style={{ margin: "0 0 4px", color: "var(--muted)", fontSize: 13 }}>
                Not in this release — first shipped in v0.4.1. The command line below works on
                every Linux release we have ever published.
              </p>
              {fallback}
            </>
          )}
        </div>

        <div style={card}>
          <p style={{ margin: "0 0 6px", fontWeight: 600 }}>Command line (macOS or Linux)</p>
          <p style={{ margin: "0 0 8px", color: "var(--muted-strong)" }}>
            Just the <code>hive</code> CLI, no GUI — good for a headless server or a machine you
            manage over SSH.
          </p>
          <pre
            style={{
              margin: "0 0 12px",
              padding: "10px 12px",
              background: "var(--surface-2)",
              borderRadius: 6,
              overflowX: "auto",
              fontSize: 13,
            }}
          >
            <code>curl -fsSL https://ohghive.com/install.sh | sh</code>
          </pre>
          <p style={{ margin: "0 0 4px", color: "var(--muted)", fontSize: 13 }}>
            Or grab a specific build directly:
          </p>
          {linuxX64 && (
            <a href={linuxX64.browser_download_url} style={btn}>
              Linux (x86_64, {mb(linuxX64.size)})
            </a>
          )}
          {linuxArm && (
            <a href={linuxArm.browser_download_url} style={btn}>
              Linux (arm64, {mb(linuxArm.size)})
            </a>
          )}
          {!linuxX64 && !linuxArm && fallback}
        </div>

        <div style={card}>
          <p style={{ margin: "0 0 6px", fontWeight: 600 }}>Windows</p>
          <p style={{ margin: "0 0 8px", color: "var(--muted-strong)" }}>
            CLI only for now — put <code>hive.exe</code> on your PATH after unzipping.
          </p>
          {windows ? (
            <a href={windows.browser_download_url} style={btn}>
              Download for Windows ({mb(windows.size)})
            </a>
          ) : (
            fallback
          )}
        </div>

        {macTauri && (
          <details style={{ marginBottom: 16 }}>
            <summary style={{ cursor: "pointer", color: "var(--muted-strong)" }}>
              Older Tauri-based Mac app (being phased out)
            </summary>
            <p style={{ marginTop: 8, color: "var(--muted)", fontSize: 13 }}>
              The native app above is the one being actively developed. This build still ships for
              now if you need it.
            </p>
            <a href={macTauri.browser_download_url} style={btn}>
              Download ({mb(macTauri.size)})
            </a>
          </details>
        )}

        <p style={{ marginTop: 24, fontSize: 13, color: "var(--muted)" }}>
          Already have Hive installed and just checking whether you&apos;re current? The app
          doesn&apos;t check for updates on its own yet for every platform — bookmark this page, or
          watch{" "}
          <a href="https://github.com/jackcanon/ohhive-releases/releases" target="_blank" rel="noreferrer">
            the releases page
          </a>
          .
        </p>
      </main>
    </>
  );
}
