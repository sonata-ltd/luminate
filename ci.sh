#!/usr/bin/env bash
# Local mirror of .github/workflows/ci.yml for the host platform: the `stable`
# job, the Linux slice of `per-crate`, `deny` and `package`. `semver`,
# `gpu` (and the macOS/Windows slices) run only on GitHub.
# Runs inside the flake's dev shell so wgpu/winit system libraries are present.
set -euo pipefail
cd "$(dirname "$0")"
# iced_test picks its headless backend from this variable; the software
# backend is deterministic on every machine (the `gpu` CI job runs wgpu).
export ICED_TEST_BACKEND=tiny-skia
run() { echo "+ $*"; nix develop --command "$@"; }

# --- stable -----------------------------------------------------------------
run cargo fmt --all -- --check
run cargo clippy --workspace --all-targets --all-features -- -D warnings
run cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" run cargo doc --workspace --no-deps --all-features
run cargo build --workspace --examples --all-features

# --- per-crate (Linux slice) -------------------------------------------------
# Each crate alone, so feature unification cannot hide a missing feature.
run cargo test -p iced_animate
run cargo check -p iced_animate --no-default-features
run cargo test -p iced_animate --all-features
run cargo clippy -p iced_animate --all-targets --all-features -- -D warnings
run cargo test -p iced_texture_cache
# One backend alone, under clippy so the cfg split stays warning-free. On
# Linux, softbuffer (behind iced_tiny_skia) needs a platform feature, so the
# tiny-skia-only step carries `x11` here; ci.yml runs plain `tiny-skia` on
# macOS/Windows and `wgpu` alone everywhere.
run cargo clippy -p iced_texture_cache --all-targets --no-default-features --features tiny-skia,x11 -- -D warnings
run cargo clippy -p iced_texture_cache --all-targets --no-default-features --features wgpu -- -D warnings
run cargo test -p iced_page_router
run cargo clippy --all-targets -p iced_page_router --no-default-features -- -D warnings
run cargo test -p iced_luminate
# iced_luminate keeps the `iced` facade, which needs an executor (`thread-pool`) and,
# on Linux, a winit platform feature; ci.yml runs plain `thread-pool,tiny-skia`
# on macOS/Windows. The doc step catches links to feature-gated items.
run cargo clippy --all-targets -p iced_luminate --no-default-features --features thread-pool,tiny-skia,x11 -- -D warnings
RUSTDOCFLAGS="-D warnings" run cargo doc -p iced_luminate --no-deps --no-default-features --features thread-pool,tiny-skia,x11

# --- deny --------------------------------------------------------------------
run cargo deny check

# --- package -----------------------------------------------------------------
# One multi-package dry run: cargo resolves the workspace's path dependencies
# against each other (a single-crate dry run of anything but iced_animate
# fails until the others are on crates.io) and verifies every package builds
# from its packaged files (fonts and OFL.txt included).
run cargo publish --dry-run -p iced_animate -p iced_texture_cache -p iced_page_router -p iced_luminate --allow-dirty
# docs.rs-equivalent builds: each crate alone with its docs.rs feature set, so
# a backend without a Linux platform feature is caught here.
RUSTDOCFLAGS="-D warnings" run cargo doc -p iced_animate --no-deps --all-features
RUSTDOCFLAGS="-D warnings" run cargo doc -p iced_texture_cache --no-deps --features wgpu,tiny-skia,image,svg,canvas
RUSTDOCFLAGS="-D warnings" run cargo doc -p iced_page_router --no-deps --all-features
RUSTDOCFLAGS="-D warnings" run cargo doc -p iced_luminate --no-deps --all-features

echo "ci: ok"
