use std::path::Path;
use std::process::Command;

/// Pinned so builds are reproducible; bump deliberately.
const CLOUDFLARED_VERSION: &str = "2026.8.3";

fn main() {
    fetch_cloudflared();
    tauri_build::build()
}

/// Bundle `cloudflared` as a Tauri resource so Tunnel setup (lib.rs `tunnel` module) needs no
/// Homebrew or manual install — matches the bar Setup already holds for Ollama. macOS
/// Apple-Silicon only for now (the app itself only ships there, ADR-010).
fn fetch_cloudflared() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("apple-darwin") {
        return;
    }
    let (cf_arch, triple) = if target.starts_with("aarch64") {
        ("arm64", "aarch64-apple-darwin")
    } else {
        ("amd64", "x86_64-apple-darwin")
    };
    let dest_dir = Path::new("resources");
    let dest = dest_dir.join(format!("cloudflared-{triple}"));
    if dest.exists() {
        println!("cargo:rerun-if-changed=build.rs");
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dest_dir) {
        println!(
            "cargo:warning=could not create resources/: {e} -- Tunnel setup will be unavailable"
        );
        return;
    }
    let url = format!(
        "https://github.com/cloudflare/cloudflared/releases/download/{CLOUDFLARED_VERSION}/cloudflared-darwin-{cf_arch}.tgz"
    );
    let tgz = dest_dir.join("cloudflared.tgz");
    let ok = Command::new("curl")
        .args(["-fsSL", "-o", tgz.to_str().unwrap(), &url])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        println!("cargo:warning=could not download cloudflared ({url}) -- Tunnel setup will be unavailable in this build");
        return;
    }
    let _ = Command::new("tar")
        .args([
            "-xzf",
            tgz.to_str().unwrap(),
            "-C",
            dest_dir.to_str().unwrap(),
        ])
        .status();
    let extracted = dest_dir.join("cloudflared");
    if extracted.exists() {
        if let Err(e) = std::fs::rename(&extracted, &dest) {
            println!("cargo:warning=could not place cloudflared binary: {e}");
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
            }
        }
    } else {
        println!("cargo:warning=cloudflared.tgz did not contain a `cloudflared` binary");
    }
    let _ = std::fs::remove_file(&tgz);
}
