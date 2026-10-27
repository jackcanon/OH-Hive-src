//! Independent Unix command cleanup prototype. This is not a sandbox.
#[cfg(unix)]
mod unix;

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    match unix::run() {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("command guardian: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(unix))]
    {
        eprintln!("Unix command guardian is not used on this platform");
        std::process::ExitCode::FAILURE
    }
}
