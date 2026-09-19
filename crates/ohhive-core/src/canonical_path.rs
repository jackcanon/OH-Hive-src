//! One canonical path representation, for Git arguments and for comparison alike.
//!
//! Lives at the crate root rather than inside `coder` because `coder` is gated on
//! `all(feature = "sandbox", feature = "hub")` and `local_hub` needs the same rule. Depends on
//! nothing but `std`, deliberately: an ungated module reaching for a gated one is the exact
//! mistake that kept `cargo check -p hive-core --all-targets` from compiling at all.

use std::path::{Path, PathBuf};

/// The largest path the ordinary Win32 parser accepts, terminating NUL included.
const MAX_PATH: usize = 260;

/// `std::fs::canonicalize`, returning a path Git can actually be handed.
///
/// On Windows `canonicalize` returns the *verbatim* (extended-length) form, `\\?\C:\x`. Git for
/// Windows is MSYS2-based and refuses that form outright -- `fatal: could not create work tree
/// dir '\\?\C:\...': Invalid argument`, which is what made five of this module's tests fail on
/// `windows-latest` while passing everywhere else. Every canonical path here either becomes a
/// `git` argument or gets compared against a path that came from somewhere else, and both uses
/// need the one plain representation.
pub(crate) fn canonical(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    if !cfg!(windows) {
        return Ok(canonical);
    }
    match canonical.to_str().and_then(simplify) {
        Some(plain) => Ok(PathBuf::from(plain)),
        None => Ok(canonical),
    }
}

/// The string half of [`canonical`], split out and taking `&str` so it runs on every platform.
///
/// Windows path prefixes parse as prefixes only *on* Windows, so an implementation written
/// against `std::path::Prefix` could never be exercised by CI on macOS or Linux -- which is
/// precisely how the bug this guards against reached main and stayed red for five pushes. Here
/// the rule is ordinary string work and the tests below run wherever CI does.
///
/// Only two verbatim shapes are simplified, because only these two name something the ordinary
/// parser resolves identically: a verbatim disk path (`\\?\C:\x` -> `C:\x`) and a verbatim UNC
/// path (`\\?\UNC\server\share` -> `\\server\share`). Anything else -- a volume GUID, a device
/// path -- is left exactly as it was. Git's refusal is a better outcome than a path we quietly
/// rewrote into pointing somewhere else.
///
/// `None` means "nothing to change", so callers keep the path they already have.
///
/// Hand-rolled rather than taking the `dunce` crate: this is the whole of what we would use from
/// it, and the behaviour worth guarding is the four cases below.
fn simplify(path: &str) -> Option<String> {
    let rest = path.strip_prefix(r"\\?\")?;
    let plain = match rest.strip_prefix(r"UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => {
            let mut rest = rest.chars();
            if !rest.next()?.is_ascii_alphabetic()
                || rest.next() != Some(':')
                || rest.next() != Some('\\')
            {
                return None;
            }
            path[r"\\?\".len()..].to_string()
        }
    };
    // Past the Win32 limit the verbatim prefix is the only thing making the path openable at all.
    // Stripping it there would trade Git's clear refusal for a failure somewhere further away.
    (plain.len() < MAX_PATH).then_some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These run on every platform on purpose. The bug they guard against was invisible to macOS
    /// and Linux CI precisely because it lived in Windows-only path parsing, so the rule is
    /// expressed as string work that every runner can execute.
    #[test]
    fn a_verbatim_disk_path_loses_its_prefix_and_a_verbatim_unc_path_becomes_a_share() {
        assert_eq!(
            simplify(r"\\?\C:\Users\runneradmin\code-repositories\ea0e"),
            Some(r"C:\Users\runneradmin\code-repositories\ea0e".to_string()),
            "the exact shape Git for Windows refused in run 35452714646"
        );
        assert_eq!(
            simplify(r"\\?\UNC\server\share\repo"),
            Some(r"\\server\share\repo".to_string())
        );
    }

    #[test]
    fn a_path_with_nothing_to_strip_is_left_alone() {
        assert_eq!(simplify(r"C:\already\plain"), None);
        assert_eq!(simplify("/home/claude/plain"), None);
        assert_eq!(simplify(""), None);
    }

    #[test]
    fn a_verbatim_path_that_is_not_a_plain_drive_or_share_is_never_rewritten() {
        // A volume GUID and a device path have no plain equivalent -- simplifying either would
        // silently change which object the path names.
        assert_eq!(simplify(r"\\?\Volume{9f3a}\repo"), None);
        assert_eq!(simplify(r"\\?\PhysicalDrive0"), None);
        // Shapes that only look like a drive.
        assert_eq!(
            simplify(r"\\?\C:"),
            None,
            "verbatim paths are fully qualified"
        );
        assert_eq!(simplify(r"\\?\4:\repo"), None, "a drive letter is a letter");
    }

    #[test]
    fn a_path_only_the_verbatim_form_can_express_keeps_it() {
        let long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH));
        assert_eq!(
            simplify(&long),
            None,
            "past MAX_PATH the prefix is load-bearing; Git's refusal beats a path Windows rejects"
        );
        let fits = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH - r"C:\".len() - 1));
        assert!(fits.len() > MAX_PATH, "the verbatim form is over the limit");
        assert!(
            simplify(&fits).is_some_and(|p| p.len() < MAX_PATH),
            "but the simplified form is under it, so it is simplified"
        );
    }
}
