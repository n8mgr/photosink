---
default: patch
---

# Build every native app through Cargo

Use `cargo xtask build` on macOS, Linux and Windows instead of separate shell and
PowerShell build scripts. Platform options retain macOS universal builds and
signing, Linux target selection, and Windows target selection, configuration and
self-contained publishing.
