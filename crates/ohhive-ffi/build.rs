fn main() {
    println!("cargo:rerun-if-env-changed=OHHIVE_BUILD_SOURCE_COMMIT");
    let stamp = std::env::var("OHHIVE_BUILD_SOURCE_COMMIT").unwrap_or_else(|_| "unstamped".into());
    assert!(
        !stamp.is_empty()
            && stamp.len() <= 128
            && stamp.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "invalid OHHIVE_BUILD_SOURCE_COMMIT"
    );
    println!("cargo:rustc-env=OHHIVE_CORE_SOURCE_COMMIT={stamp}");
}
