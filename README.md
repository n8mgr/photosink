# photosink

Exports Apple Photos, Google Takeout, OneDrive, and Dropbox into one tree: originals, RAW
alternates, edited renders, Live Photo videos, and an XMP metadata sidecar per asset. Apple originals not stored locally are downloaded from iCloud
through PhotoKit. Files already exported from another source are detected by hash and
not written twice.

## Build

Each platform has its own native app over the same Rust core:

- **macOS** (SwiftUI): `apps/macos/build.sh` builds the bridge, generates the Swift
  bindings, and assembles `apps/macos/build/Photosink.app`.
- **Linux** (GTK 4 + libadwaita): `cargo run --release --manifest-path apps/linux/Cargo.toml`.
- **Windows** (WinUI 3): `apps/windows/build.ps1`; see `apps/windows/README.md`.

The Rust library is the root crate. `bridge` is the window model every app renders:
settings, tabs, runs, the status grid and its filters, and the Dates picker, exported
with UniFFI.

## Requirements

- Apple Photos export needs macOS, and the library must be the system photo library
  (PhotoKit only exposes that one). OneDrive, Dropbox, Takeout and the maintenance tools run on
  macOS, Windows and Linux.
- Full Disk Access for the app, to read `Photos.sqlite`. Photos access is requested on
  first run.

## Use

Pick the export folder and the path template at the top of the window, choose a source,
fill in its argument, and run. Settings are stored in
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

Sources: Apple Photos takes the library path, defaulting to
`~/Pictures/Photos Library.photoslibrary`, which only changes where metadata is read
since PhotoKit always serves the system library. OneDrive takes a folder under the drive
root, empty for the whole drive. Google Takeout takes a directory of archives, a single
archive, or an extracted tree.

### Takeout

Request a Google Photos export at <https://takeout.google.com> and point the app at the
directory holding the `.zip` or `.tgz` archives, a single archive, or an already
extracted tree. Archives are read twice: once to list entries and load the JSON sidecars,
so pairs split across archives still match, then once more streaming each media file into
`export_dir/.tmp` while hashing it, after which it is moved into place. Each media file is paired with its JSON sidecar
(`X.jpg.json`, `X.jpg.supplemental-metadata.json`, truncated variants, and the `(1)`
duplicate suffix), which supplies take time (falling back to the upload time), GPS,
description, tagged people, and favorite. `X-edited.jpg` is attached to `X.jpg` as the edited render. Album folders
(anything not named `Photos from NNNN`) become the album.

### OneDrive

A built-in Azure app registration is used; set `onedrive_client_id` in the config to use
your own (a public client with "Allow public client flows" enabled and personal accounts
supported). The first run shows a device-login code in the log; the token is cached
at `~/Library/Application Support/photosink/onedrive_token.json`. Listing uses the Graph delta
API, so reruns only see new or changed files. The cursor is stored in the state DB and
only advanced after an error-free run, keyed by folder. Take time, camera, and GPS come from the Graph
`photo` and `location` facets; items without a `photo` facet use the file's original
creation time, then the upload time. EXIF fills GPS and camera in the sidecar.

### Dropbox

Dropbox needs your own app key; there is no shared one. Create an app at
https://www.dropbox.com/developers/apps with scoped access, give it the
`files.metadata.read` and `files.content.read` permissions, add
`http://localhost:53917/dropbox` as a redirect URI, and put its app key in the config as
`dropbox_app_key`. The first run opens the browser to approve access (PKCE, no app secret)
and catches the redirect on that port; the token is cached at
`~/Library/Application Support/photosink/dropbox_token.json` and refreshed as needed.
The Dropbox tab takes a folder (empty for everything) and lists it recursively; the
cursor is stored per folder and only advanced after an error-free run, so reruns only see
new or changed files. Dropbox's listing carries no capture date or location, so the taken
date comes from the file's own EXIF, then the file's modified time, and place fields stay
empty.

### Immich

Upload the export tree with the Immich CLI. It reads `IMG.xmp` next to `IMG.CR3` for date,
GPS, description, keywords as tags, and rating. Edited renders are separate assets to
Immich, so leave `edits` off if you don't want them beside the originals.

`filename` names the files (default `{stem}`, the original name); it takes the same fields, and
Consolidate renames when it changes. `date_source = "file"` prefers the file's own EXIF date over
the source's; `date_zone` is the clock the folder date is read in: `asset` (as recorded), `local`,
`utc`, or an offset like `+02:00`. A `[filter]` table limits a run: `media = "photos"|"videos"`,
`favorites = true`, `from`/`to` dates, and `albums`/`keywords` lists where any match passes;
assets without any date pass a date range. Consolidate and Dates ignore the filter.

The path editor lists every field as a clickable chip, previews the result against an
exported asset (or a sample photo), and refuses to save a template with an unknown field.
`path` fields: `take.year`, `take.month`, `take.day`, `take.date`, `place.name`,
`place.city`, `place.state`, `place.country`, `keyword:N`, `album:N`, `label:N`, `people:N`,
`title`, `uuid`, `stem`, `type`. `{a|b}` uses the first non-empty alternative. Missing
fields produce an empty segment, which is dropped.

### Clean

**Clean** walks the export folder against the state DB. Rows whose file is gone are forgotten;
an asset left with no files also loses its sidecar and its folder claim, so the next source run
exports it again. Sidecars no asset claims are deleted. With **Also delete files the state
doesn't reference** on, unreferenced media files are deleted too. Dry run lists everything
without changing anything; Remove asks for confirmation first.

### Dry run

Tick **Dry run** next to Run to walk the source without downloading, writing or recording
anything: every resource that would be exported is logged with its destination, and the
summary counts them as planned. Hash-level duplicates can't be predicted without the bytes,
and two new assets that would share a name in one folder both plan the bare name. Consolidate
with Dry run lists the moves it would make.

### Dates

The path template's date is the source's taken date. OneDrive and Takeout only report one
when they extracted it themselves; otherwise the exporter reads the downloaded file
(EXIF DateTimeOriginal, then DateTimeDigitized, then ModifyDate, or the `mvhd` creation time
of a video) and only then falls back to the upload time. The date used is stored under
`taken:<uuid>` in the state DB so later runs keep it.

**Dates → Find** lists every asset whose folder, name or original filename contains the text,
whatever its dates, for manual corrections. With the box empty, **Scan** lists every asset whose
folder date disagrees with its file: the current
date next to the file's create, original and modify dates. Pick one per asset, or type a
date, then **Apply** stores the override, rewrites the sidecar and the embedded packet, and
moves the asset to the folder the template now produces. An asset whose camera clock was
wrong usually shows an implausible original date and a believable modify date from the
editor that last saved it.

### Consolidate

Consolidate re-applies the current path template to everything already exported. Each
asset's JSON sidecar is read back, the template rendered, and the asset's files and
sidecar moved when the folder differs, with the state DB rows updated so later runs still
skip and dedup them. A stem already taken in the target folder gets the asset's uuid
prefix appended, as with new exports. Assets without a sidecar are left alone. Emptied
folders are removed. Change the template, consolidate, and `2019/05/Paris` and
`2019/05/Paris 1er` collapse into `2019/05/Paris`.

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

The sidecar is written unless `sidecar = false`. The same XMP packet is embedded in each
media file unless `embed = false`: an APP1
segment in JPEG, an `iTXt` chunk in PNG, IFD0 tag 700 in TIFF-based RAW and DNG, a `mime`
item in HEIC, and a top-level `uuid` box in MOV, MP4, and CR3 (with Canon's `CTBO` table
updated). Existing packets are rewritten in place when the new one fits in the padding;
otherwise the packet is appended and the old slot freed. A file shared by two assets through
dedup is skipped. Formats without a writer (RAF, BigTIFF) keep only the sidecar. Consolidate
and Dates read the asset back from the sidecar, or from the embedded packet when there is
none, so at least one of the two should stay on.

Files are blake3-hashed; a file whose hash was already exported is not written again.
The hash recorded is that of the bytes received from the source, before embedding.
State lives in `~/Library/Application Support/photosink/state.sqlite`.

## Design

`photo` holds the `Asset` model, EXIF reading, and place decoding. `sources` holds the
`Source` trait, resource kinds, and one module per source. `export` and `state` are the
sink. `lib.rs` exposes `App` with one method per source and a typed `Event` progress
callback. `bridge` runs those on a worker thread, groups the events into each tab's rows,
and tells the platform UI what to re-read at most every 100 ms.

1. `sources::apple_photos::library` copies `Photos.sqlite` (plus WAL) to a temp dir and loads one `Asset`
   per non-trashed row: filename, title, description, take time with timezone, GPS,
   favorite, keywords, user albums, and place decoded from `ZREVERSELOCATIONDATA`,
   which is an NSKeyedArchiver archive on macOS and a `geoPlaceResult` protobuf on
   iOS-origin libraries. Assets with GPS but no reverse-geocode fall back to the nearest
   GeoNames city embedded via `reverse_geocoder`: city and name are the city, state is
   the admin1 region, country is the ISO code. Automatic scene labels and recognised
   people come from the search index at `database/search/leo.sqlite`, ordered by
   confidence with the most specific entry first among ties. Keyword and album join tables are found by
   column suffix at runtime because their numeric names change between Photos versions.
2. `sources::apple_photos::photokit` authorizes, enumerates assets, lists each asset's resources, and writes a
   resource to a file with network access allowed, which downloads missing originals.
3. `export` renders the `path` template per asset, then for each resource: skip if
   already in the state DB, write to `export_dir/.tmp`, blake3-hash, drop and record if
   the hash exists, otherwise move into place without clobbering. A `<stem>.xmp` sidecar
   carries the metadata in standard namespaces (dc, xmp, exif, Iptc4xmpExt, tiff) that
   Immich, Lightroom, and digiKam read, plus the full asset as JSON under a `photosink`
   namespace so Consolidate can rebuild the path; an asset whose files were all duplicates claims no stem and
   gets no sidecar. `photo::embed` then writes the packet into the asset's files, one
   container-specific writer each (`jpeg`, `png`, `tiff`, `isobmff`); only `meta` in HEIC
   needs the file rewritten, and there every `iloc` extent is rebased by the growth. Apple Photos and OneDrive run `concurrency` worker threads pulling
   assets from a queue; Takeout is a single stream.

4. `sources::onedrive` implements the same `Source` trait as PhotoKit, downloading
   `/content` with retry on 429/503 and refreshing the OAuth token when it expires.
   `sources::takeout` drives the
   exporter's placement directly: one pass to index names and sidecars, one pass streaming
   each entry through a hashing writer into `.tmp` and then into place.

`PHOTOS_LIBRARY=<path> cargo test loads_real_library -- --ignored --nocapture` dumps what the
loader sees for a library, including iOS Simulator libraries under
`~/Library/Developer/CoreSimulator/Devices/*/data/Media/PhotoData`.

## License

GPL-3.0-or-later; see `LICENSE`. The embedded metadata writers follow ExifTool, which is offered under the GPL.
