# Changelog
## 0.1.2 (2026-11-10)

### Features

#### Export automatically in the background

Enable automatic exports directly on each configured source and choose the check
interval in Settings. OneDrive and Dropbox reuse saved credentials, and
changed media exports preserve earlier versions.
Existing exports are used as the starting point for change tracking without downloading
them again.

Photosink can continue exporting after its window closes and optionally start at login.

### Fixes

#### Keep earlier exports tracked during maintenance

Earlier versions remain recorded so Clean preserves them and Consolidate moves
them with their asset. Exports also recreate deleted destination folders and
handle duplicate detection safely when revisions export concurrently.
Consolidate handles overlapping revision filenames, and Dates and Consolidate
can use metadata from earlier versions when the latest export is missing.
Failed destination creation no longer reserves an unusable export location.

## 0.1.1 (2026-11-04)

### Features

#### Build and open the native app with one command

Use `cargo build-app --open` to build and launch Photosink on macOS, Linux and
Windows. The `build-app` alias supports the same options as `cargo xtask build`,
including universal builds and Windows publishing. The app opens only after the
build succeeds.

#### Add Dropbox as a source

Dropbox folders can be exported like OneDrive: the first run signs in through the browser, and later runs only fetch what changed. It needs your own Dropbox app key in `dropbox_app_key`; see the README for setup.

#### Native apps for macOS, Windows and Linux

Photosink now ships as a native app on each platform over one Rust core: SwiftUI on macOS, WinUI 3 on Windows and GTK 4 with libadwaita on Linux. Each uses its platform's own window structure, with tabs for every source and maintenance task, a filterable results table, and a separate settings window.

#### Open export rows in Photos

Right-click an Apple Photos asset row and choose Open in Photos to reveal the original item, including videos that failed to download.

### Fixes

#### Show all statuses in the grid filter

The grid filter includes every possible status, even before matching rows appear, so filters can be set before a run.

#### Build every native app through Cargo

Use `cargo xtask build` on macOS, Linux and Windows instead of separate shell and
PowerShell build scripts. Platform options retain macOS universal builds and
signing, Linux target selection, and Windows target selection, configuration and
self-contained publishing.

#### Applying dates no longer stops at the first failing asset

When one chosen asset had no metadata or an unreadable date, Apply stopped there and the assets already redated kept their new dates but were never moved to their new folders. Each failure is now listed on its row and the rest are redated and moved.

#### Native date pickers and locale formatting

Dates and counts now follow your system's locale on every platform, and the Dates picker's manual date uses each platform's native date and time picker. A manual date is applied as wall-clock time in the asset's existing offset; typing an explicit RFC 3339 offset is no longer supported.

#### Add native release installers

Windows releases include a setup installer and Linux releases include DEB packages
with runtime dependencies. macOS releases require Developer ID signing and verified,
stapled notarization for the universal DMG.

#### Retry failed network downloads

Apple Photos, OneDrive and Dropbox file downloads now retry up to three times after the first failure, with increasing delays and a fresh temporary file for each attempt.

#### Takeout sidecars without edits, and library warnings in the run

Google Takeout imports with edits turned off now write the sidecar for photos that also have an edited copy; before, those photos were exported without their metadata. Apple Photos runs now show a warning in the run header when the library has no keyword table, album table or search index, instead of dropping the warning silently.

#### Fix Windows export maintenance

Find exported assets when consolidating or fixing dates on Windows, and preserve live sidecars when paths use mixed separators.
