# Sync WIT deps (copies web3-runtime into shepherd-cow/deps)
sync-wit:
    rm -rf wit/shepherd-cow/deps/web3-runtime
    cp -r wit/web3-runtime wit/shepherd-cow/deps/web3-runtime

# Build the host runtime
build-runtime: sync-wit
    cargo build -p nxm-engine

# Build the example WASM module
build-module:
    cargo build --target wasm32-wasip2 --release -p example

# Build everything
build: build-runtime build-module

# Build the module then run the runtime with it
run: build-module build-runtime
    cargo run -p nxm-engine -- target/wasm32-wasip2/release/example.wasm

# Check the entire workspace
check: sync-wit
    cargo check --target wasm32-wasip2 -p example
    cargo check -p nxm-engine
