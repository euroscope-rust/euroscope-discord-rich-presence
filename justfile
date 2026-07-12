set shell := ["powershell.exe", "-NoLogo", "-Command"]

# List available commands
default:
  just --list

alias f := fmt
# Auto format code
fmt:
  cargo +nightly fmt
[private]
ci-fmt:
  cargo +nightly fmt --check

alias l := lint
# Lint code
lint:
  cargo clippy
[private]
ci-lint:
  $env:RUSTFLAGS = "-Dwarnings"; just lint

alias b := build
# Build (debug) for the 32-bit EuroScope target
build:
  cargo build
[private]
ci-build:
  $env:RUSTFLAGS = "-Dwarnings"; cargo build

alias t := test
# Run tests
test:
  $env:EUROSCOPE_PLUGIN_DELAYLOAD = "1"; cargo test
[private]
ci-test: test

# Cleanup rust build directory
clean:
  cargo clean
