# AGENTS.md

Photosink exports Apple Photos, OneDrive, Dropbox and Google Takeout into one folder tree
with XMP metadata, and maintains that tree (consolidate, clean, fix dates). It ships as a
native app per platform over one Rust core.

## Layout

| Path | What |
|---|---|
| `src/` | `photosink` crate: sources, export, state DB, EXIF/XMP writers, consolidate, clean, redate. Emits typed `Event`s through a `Progress` callback. |
| `bridge/` | `photosink-bridge`: the window model every app renders, exported with UniFFI 0.31. Settings, tab catalogue, runs, the status grid (grouping, states, filters) and the Dates picker. |
| `apps/macos/` | SwiftUI app (SwiftPM, macOS 26). `build.sh` builds the bridge, generates Swift bindings and assembles `build/Photosink.app`. |
| `apps/linux/` | GTK 4 + libadwaita app; its own Cargo workspace using `bridge` as a crate. |
| `apps/windows/` | WinUI 3 app (C#, .NET 10). `build.ps1` builds the bridge, runs `uniffi-bindgen-cs` and the .NET build. |
| `.changeset/` | Pending changelog entries for knope. |

## Rules

- **Logic lives in Rust.** Anything more than presentation (labels, validation, summaries,
  row grouping, filter state, run labels, confirmations) belongs in `bridge`, so the three
  apps stay thin and say the same thing. If an app needs something Core doesn't expose,
  add it to `bridge` rather than computing it in Swift, Rust-GTK or C#.
- **Native structure, stock controls.** Each app uses its platform's own window structure:
  macOS sidebar + toolbar + window subtitle + Settings scene; GNOME NavigationSplitView +
  HeaderBar + PreferencesDialog; Windows NavigationView + CommandBar + Settings page. No
  hand-drawn bands, custom tab strips or restyled controls.
- **Events are typed.** Never add free-text progress messages; add an `Event` variant and
  handle it in `bridge/src/grid.rs` (or the Dates model). The run log on disk is the
  `Display` of the same events.
- **Apple Photos is macOS-only.** PhotoKit code is behind `cfg(target_os = "macos")`; the
  core, bridge and every other source must build on Windows and Linux.
- **Never touch the user's real data in tests.** Tests use temp dirs and must not read or
  write `~/Library/Application Support/photosink`, the Photos library, or the export folder.
- **Comments:** no section-divider comments and no comments justifying design choices; keep
  doc comments short and only where the code isn't obvious.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check                    # licenses (GPL-3.0 compatible), advisories, sources

# Linux app (needs libgtk-4-dev and libadwaita-1-dev on Linux, or brew gtk4 libadwaita)
cargo clippy --manifest-path apps/linux/Cargo.toml --all-targets -- -D warnings

# macOS app (Xcode 26+)
apps/macos/build.sh                 # this Mac's arch, ad-hoc signed
UNIVERSAL=1 apps/macos/build.sh     # arm64 + x86_64

# Windows app (PowerShell, Rust MSVC, .NET 10 SDK, uniffi-bindgen-cs v0.11.1+v0.31.0)
apps/windows/build.ps1
```

After changing an exported `bridge` type, rebuild each app so its generated bindings
match; Swift and C# bindings are generated at build time and are not committed.

## Changes and releases

Every user-visible change needs a changeset. Run `knope document-change`, or add
`.changeset/<short_name>.md`:

```md
---
default: patch
---

# One-line summary

What changed and why it matters to someone using the app.
```

Use `minor` for new features and `major` for breaking changes. Merges to `master` open
(or update) a "chore: prepare release" PR; merging that PR tags the version, builds and
signs the apps on all three platforms, and publishes the GitHub release. Versions are kept
in sync by `knope.toml` across the crates, `Info.plist` and the `.csproj`.
