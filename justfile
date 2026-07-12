[windows]
set shell := ["powershell.exe", "-NoLogo", "-Command"]

# List available commands
default:
  just --list

# Auto format code
fmt:
  cargo +nightly fmt

alias l := lint
# Lint code
lint:
  cargo clippy

alias b := build
# Build (debug) for the 32-bit EuroScope target
build:
  cargo build

alias t := test
# Run tests
test:
  $env:EUROSCOPE_PLUGIN_DELAYLOAD = "1"; cargo test

# Cleanup rust build directory
clean:
  cargo clean
