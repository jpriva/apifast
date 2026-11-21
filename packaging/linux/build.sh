#!/usr/bin/env sh
# Builds the Linux packages of apifast into target/packages: a .deb and an
# .rpm, each with the interface (apifast-gui) and the command line (apifast).
#
# Needs cargo-deb and cargo-generate-rpm:
#   cargo install --locked cargo-deb cargo-generate-rpm
#
# The binaries need the glibc of the system they are built on: release
# packages are built on the oldest supported one (Ubuntu 22.04).
set -eu

cd "$(dirname "$0")/../.."
out=target/packages
mkdir -p "$out"

cargo build --release --locked -p apifast-cli -p apifast-gui
cargo deb --no-build --no-strip -p apifast-gui --output "$out/"
cargo generate-rpm -p crates/apifast-gui --output "$out/"

ls -l "$out"
