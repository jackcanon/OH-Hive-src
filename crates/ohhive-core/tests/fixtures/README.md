# Sandbox test fixtures

`sandbox-echo.wasm` — a minimal WASI Preview 2 component used by
`tests/sandbox_e2e.rs` to exercise `Sandbox::run` against a real compiled
component instead of only the policy/enforcement unit tests in
`src/sandbox.rs`.

Source (not part of the workspace — this is a throwaway fixture, rebuilt
locally and committed as a binary so CI never needs a `wasm32-wasip2` target
installed just to run this one test):

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
