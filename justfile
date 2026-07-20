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
  cargo clippy --all-targets
[private]
ci-lint:
  $env:RUSTFLAGS = "-Dwarnings"; just lint

alias b := build
# Build (debug)
build:
  cargo build
alias br := build-release
# Build (release)
build-release:
  cargo build --release
[private]
ci-build:
  $env:RUSTFLAGS = "-Dwarnings"; just build-release


alias t := test
# Run tests
test:
  $env:EUROSCOPE_PLUGIN_DELAYLOAD = "1"; cargo test
[private]
ci-test:
  $env:RUSTFLAGS = "-Dwarnings"; just test

# Cleanup rust build directory
clean:
  cargo clean
