# photosink

Exports Apple Photos, Google Takeout, OneDrive, and Dropbox into one folder tree with
originals, RAW alternates, optional edited renders, Live Photo videos, and XMP metadata.
Downloads missing Apple Photos originals from iCloud and skips duplicate files across
sources. Includes tools to reorganize exports, correct dates, and clean up missing or
unreferenced files.

## Install

Download an installer from [GitHub Releases](https://github.com/n8mgr/photosink/releases):

- **macOS 26+**: open the signed, notarized `.dmg` and drag Photosink to Applications.
- **Windows x64**: run `photosink_windows_x64_setup.exe` to install for your user.
- **Ubuntu 24.04+**: download the `.deb` for your architecture and run
  `sudo apt install ./photosink_linux_<arch>.deb` (`x86_64` or `aarch64`).

Portable Windows `.zip` and Linux `.tar.gz` archives are also available.
Release signing setup is documented in [.github/RELEASING.md](.github/RELEASING.md).

## Build

From the repository root, build the app for your platform with:

```sh
cargo xtask build
```

`cargo build-app` is a shorter alias for the same build. Add `--open` to launch
the app after the build succeeds:

```sh
cargo build-app --open
```

- **macOS** (SwiftUI, Xcode 26+): builds the bridge, generates the Swift bindings,
  and assembles `apps/macos/build/Photosink.app`. Use `--universal` for arm64 and
  x86_64 (install both Rust targets first). Signing is ad-hoc by default; set
  `SIGN_IDENTITY` to sign with a certificate, hardened runtime and entitlements.
- **Linux** (GTK 4 + libadwaita): builds `apps/linux/target/release/photosink`.
  Requires `libgtk-4-dev` and `libadwaita-1-dev`. Use `--target <TRIPLE>` to build
  for another Linux architecture with its toolchain and libraries installed.
- **Windows** (WinUI 3): builds the bridge, generates the C# bindings, and builds
  the app. Use `--publish` for self-contained output; see `apps/windows/README.md`.

Builds default to Release. `cargo xtask build --help` lists the platform options.

## Requirements

- Apple Photos export needs macOS and the system photo library. Grant the app Full Disk
  Access to read library metadata. Photos access is requested on first run.
- OneDrive, Dropbox, Takeout and the maintenance tools run on macOS, Windows and Linux.

## Use

Pick the export folder and path template, choose a source, enter its library, folder or
archive path, and run. On macOS, settings are stored in
`~/Library/Application Support/photosink/config.toml`:

```toml
export_dir = "/Users/me/Pictures/Export"
path = "{take.year}/{take.month}/{place.name}"
concurrency = 4
edits = false
```

By default only originals, RAW alternates, and Live Photo videos are exported. `edits`
adds the rendered edit and the `.AAE` adjustment data, which only Apple Photos (or the
app that made the edit) can reapply.

### Automatic exports

Enable **Export automatically** on each source you want checked. The default interval is
15 minutes and can be set from 1 minute to 24 hours in **Settings → Automation**.
Library and folder selections are saved between launches.
Automatic runs use the saved export settings, always write changes rather than performing
a dry run, and wait for any current export or maintenance operation to finish.

**Keep running after close** leaves Photosink checking in the background. On macOS, use
the menu bar item to reopen the window or open Settings. With this option enabled,
**Quit Photosink** (Command-Q) closes the windows and keeps background exports running;
choose **Quit Photosink Completely** from the menu bar to stop the app. Windows provides
a notification-area icon, and Linux reopens the existing window when launched again.
**Open at login** starts the app when you sign in. Both options are off by default.
Exiting the app completely stops automatic checks; it does not run as a system service.

Run each source manually once to grant Photos access or sign in. OneDrive and Dropbox
reuse saved refresh tokens, so they normally do not need another sign-in. Automatic runs
report missing or revoked access in the source's log without opening a sign-in prompt.
Takeout checks the configured local archives or extracted directory; it does not request
new Google exports.

Apple Photos, OneDrive and Dropbox track source revisions to export changed media as well
as new items. For existing exports without saved revisions, the first check records a
baseline without downloading the media; only subsequent revision changes are detected.
Previous exported versions are retained, and source deletions do not delete
local exports. Metadata is refreshed for assets visited by a check. Apple Photos checks
the system photo library; only one library or folder per source can be selected at a time.
The asset's base XMP sidecar describes its current metadata. Suffixed media versions
share that sidecar; enable embedded metadata for tools that require metadata in each file.

### Apple Photos

Select the system photo library. The default path is
`~/Pictures/Photos Library.photoslibrary`. The path selects where metadata is read;
media always comes from the system photo library.

### Takeout

Request a Google Photos export at <https://takeout.google.com> and point the app at the
directory holding the `.zip` or `.tgz` archives, a single archive, or an already
extracted tree. JSON sidecars supply capture time, GPS, description, tagged people, and
favorite status, including when media and sidecars are split across archives.
`X-edited.jpg` is attached to `X.jpg` as the edited render. Album folders (anything not
named `Photos from NNNN`) become the album.

### OneDrive

Enter a folder under the drive root, or leave it empty for the whole drive. The first
run shows a device-login code in the log. Later runs check for new or changed files.

Photosink includes an Azure app registration. To use your own, create a public client
with "Allow public client flows" enabled and personal accounts supported, then set
`onedrive_client_id` in the config to its client ID. On macOS, the token is cached at
`~/Library/Application Support/photosink/onedrive_token.json`.

### Dropbox

Create a Dropbox app at
https://www.dropbox.com/developers/apps with scoped access, give it the
`files.metadata.read` and `files.content.read` permissions, add
`http://localhost:53917/dropbox` as a redirect URI, and put its app key in the config as
`dropbox_app_key`. The first run opens the browser to approve access; no app secret is
needed. On macOS, the token is cached at
`~/Library/Application Support/photosink/dropbox_token.json`.

Enter a folder, or leave it empty for everything. Subfolders are included. Later runs
check for new or changed files. Capture dates come from EXIF, falling back to the file's
modified time. Place fields stay empty.

### Immich

Upload the export tree with the Immich CLI. It reads `IMG.xmp` next to `IMG.CR3` for date,
GPS, description, keywords as tags, and rating. Edited renders are separate assets to
Immich, so leave `edits` off if you don't want them beside the originals.

### Paths and filters

`filename` sets the output filename (default `{stem}`, the original name) and uses the
same fields as `path`. Consolidate applies filename changes to existing exports.

`date_source = "file"` prefers the file's own EXIF date over the source's. `date_zone`
sets the time zone used in path dates: `asset` (as recorded), `local`, `utc`, or an
offset like `+02:00`.

A `[filter]` table limits a run: `media` accepts `"photos"` or `"videos"`,
`favorites = true` selects favorites, `from`/`to` set a date range, and
`albums`/`keywords` lists accept any match. Assets without dates pass a date range.
Consolidate and Dates ignore the filter.

The path editor lists every field as a clickable chip, previews the result against an
exported asset (or a sample photo), and refuses to save a template with an unknown field.
`path` fields: `take.year`, `take.month`, `take.day`, `take.date`, `place.name`,
`place.city`, `place.state`, `place.country`, `keyword:N`, `album:N`, `label:N`, `people:N`,
`title`, `uuid`, `stem`, `type`. `{a|b}` uses the first non-empty alternative. Missing
fields produce an empty segment, which is dropped.

### Clean

**Clean** removes records for missing files and deletes sidecars that no asset uses.
Assets with no remaining files can be exported again on the next source run. With
**Also delete files the state doesn't reference** on, unreferenced media files are deleted
too. Dry run lists everything without changing anything; Remove asks for confirmation
first.

### Dry run

Tick **Dry run** next to Run to list planned exports and their destinations without
downloading or writing files. It cannot detect duplicates by content or resolve filename
collisions between new assets. Consolidate with Dry run lists the moves it would make.

### Dates

The path template uses the source's capture date, falling back to the file's metadata
and then the upload or modified date. Date corrections are saved for later runs.

**Dates → Find** searches folder names, filenames and original filenames for manual
corrections. With the box empty, **Scan** lists every asset whose
folder date disagrees with its file. It shows the current date alongside the file's
create, original and modify dates. Pick one per asset, or type a date, then **Apply**
saves the correction, updates the XMP metadata, and moves the asset to the folder
produced by the template.

### Consolidate

**Consolidate** applies the current folder and filename templates to existing exports.
It moves files and sidecars, resolves filename collisions, and removes emptied folders.
Assets without Photosink metadata are left alone.

## Output

Per asset, under `export_dir/<path>/`:

| resource          | file                 |
|-------------------|----------------------|
| original          | `IMG_1.HEIC`         |
| RAW alternate     | `IMG_1.DNG`          |
| edited render     | `IMG_1_edited.jpeg` (with `edits`) |
| adjustments       | `IMG_1.AAE` (with `edits`) |
| Live Photo video  | `IMG_1.MOV`          |
| metadata          | `IMG_1.xmp` (with `sidecar`) |

The XMP sidecar is written unless `sidecar = false`. Metadata is also embedded in JPEG,
PNG, TIFF-based RAW, DNG, HEIC, MOV, MP4, and CR3 unless `embed = false`. Files shared by
multiple assets are skipped when embedding. Formats without an embedded metadata writer
(RAF, BigTIFF) keep only the sidecar. Keep sidecars or embedded metadata enabled to use
Consolidate and Dates.

Duplicate files are detected by content and skipped, including across sources.
On macOS, state lives in `~/Library/Application Support/photosink/state.sqlite`.

## License

GPL-3.0-or-later; see `LICENSE`. The embedded metadata writers follow ExifTool, which is offered under the GPL.
