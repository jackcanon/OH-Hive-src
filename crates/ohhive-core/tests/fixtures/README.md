# Sandbox test fixtures

Minimal WASI Preview 2 components used by `tests/sandbox_e2e.rs` to exercise
`Sandbox::run` against real compiled components instead of only the
policy/enforcement unit tests in `src/sandbox.rs`. Neither is part of the
workspace — both are throwaway fixtures, rebuilt locally and committed as
binaries so CI never needs a `wasm32-wasip2` target installed just to run
these tests.

## `sandbox-echo.wasm`

Proves a plain scratch-dir write lands correctly.

```rust
fn main() {
    std::fs::write("sandbox-echo-output.txt", "hello from the sandbox\n")
        .expect("write into preopened scratch dir");
    println!("sandbox-echo: wrote output file");
}
```

Rebuild with:

```
cargo new --bin sandbox-fixture-build
# paste the source above into src/main.rs
rustup target add wasm32-wasip2
cd sandbox-fixture-build && cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/sandbox-fixture-build.wasm \
   <this dir>/sandbox-echo.wasm
```

## `sandbox-inputs-echo.wasm`

Proves `Sandbox::run`'s optional `inputs_dir` is reachable read-only at `/in`
(used by `artifact_get` to hand a fetched artifact to a later `exec_wasm`
call) — reads a staged file, copies it into scratch, and separately confirms
a write attempt into `/in` is rejected.

```rust
fn main() {
    let staged = std::fs::read_to_string("/in/hello.txt")
        .expect("read staged input from the read-only /in mount");
    std::fs::write("from-in.txt", &staged).expect("write into the writable scratch mount");

    let write_attempt = std::fs::write("/in/should-fail.txt", b"nope");
    let verdict = match write_attempt {
        Ok(()) => "BUG: write to /in succeeded",
        Err(_) => "ok: write to /in was rejected",
    };
    std::fs::write("write-attempt-result.txt", verdict).expect("write verdict to scratch");
}
```

Rebuild the same way, substituting `sandbox-fixture-build2` /
`sandbox-inputs-echo.wasm`.
