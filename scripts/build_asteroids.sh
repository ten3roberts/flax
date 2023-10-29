set -e
cargo build --release --package asteroids --target wasm32-unknown-unknown --manifest-path=asteroids/Cargo.toml

mv ./target/wasm32-unknown-unknown/release/asteroids.wasm ./asteroids/public
