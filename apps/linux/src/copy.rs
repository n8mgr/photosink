use gtk::glib;
use photosink_bridge::catalog::{Blocker, Confirmation, RunArgs, RunTitle, Tab};
use photosink_bridge::dates::DateKey;
use photosink_bridge::grid::{CleanOutcome, CleanReason, Fact, RowState, SignIn};
use photosink_bridge::settings::{
    DateSource, FieldGroup, Media, Metadata, Settings, SettingsProblem, TemplateKind, ZoneMode,
};
use photosink_bridge::{CoreError, StatKind, Status};

pub const DATE_SOURCES: [DateSource; 2] = [DateSource::Source, DateSource::File];
pub const ZONES: [ZoneMode; 4] = [
    ZoneMode::Asset,
    ZoneMode::Local,
    ZoneMode::Utc,
    ZoneMode::Fixed,
];
pub const MEDIA: [Media; 3] = [Media::All, Media::Photos, Media::Videos];
pub const METADATA: [Metadata; 3] = [Metadata::Both, Metadata::Embed, Metadata::Sidecar];
pub const DATE_KEYS: [DateKey; 3] = [DateKey::Create, DateKey::Original, DateKey::Modify];
pub const FIELD_GROUPS: [FieldGroup; 5] = [
    FieldGroup::Date,
    FieldGroup::Place,
    FieldGroup::Tags,
    FieldGroup::Camera,
    FieldGroup::Other,
];

pub const DATE_SOURCE_HINT: &str = "Falls back to the other, then the upload time";
pub const ZONE_HINT: &str = "The zone the folder date is read in";
pub const OFFSET_PLACEHOLDER: &str = "+02:00";
pub const FAVORITES: &str = "Favourites only";
pub const LIST_PLACEHOLDER: &str = "Comma separated; any match";
pub const EDITS_OFF: &str = "Originals";
pub const EDITS_ON: &str = "Originals + edits";
pub const EDITS_HINT: &str = "Edits are the rendered version plus the .AAE adjustment data";
pub const METADATA_HINT: &str =
    "The XMP packet written into each file, to a .xmp beside it, or both";
pub const FOLDER_PLACEHOLDER: &str = "{take.year}/{take.month}";
pub const FILENAME_PLACEHOLDER: &str = "{stem}";

pub const CONFIRM_CLEAN_TITLE: &str = "Clean the Export";
pub const CONFIRM_CLEAN_OK: &str = "Delete";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Import,
    Maintain,
}

impl Group {
    pub fn name(self) -> &'static str {
        match self {
            Group::Import => "Import",
            Group::Maintain => "Maintain",
        }
    }
}

pub fn group(tab: Tab) -> Group {
    match tab {
        Tab::ApplePhotos | Tab::OneDrive | Tab::Dropbox | Tab::GoogleTakeout => Group::Import,
        Tab::Consolidate | Tab::Clean | Tab::Dates => Group::Maintain,
    }
}

pub fn tab_name(tab: Tab) -> &'static str {
    match tab {
        Tab::ApplePhotos => "Apple Photos",
        Tab::OneDrive => "OneDrive",
        Tab::Dropbox => "Dropbox",
        Tab::GoogleTakeout => "Google Takeout",
        Tab::Consolidate => "Consolidate",
        Tab::Clean => "Clean",
        Tab::Dates => "Dates",
    }
}

pub fn arg_label(tab: Tab) -> &'static str {
    match tab {
        Tab::ApplePhotos => "Library",
        Tab::OneDrive | Tab::Dropbox => "Folder",
        Tab::GoogleTakeout => "Takeout",
        Tab::Dates => "Find",
        Tab::Consolidate | Tab::Clean => "",
    }
}

pub fn hint(tab: Tab) -> &'static str {
    match tab {
        Tab::ApplePhotos => {
            "Exports originals and metadata from the Photos library through PhotoKit."
        }
        Tab::OneDrive => {
            "Signs in with a device code the first time, then exports what changed since the last run."
        }
        Tab::Dropbox => {
            "Signs in through the browser the first time, then exports what changed since the last run."
        }
        Tab::GoogleTakeout => "Reads the archives in place; nothing needs extracting first.",
        Tab::Consolidate => {
            "Moves everything already exported into the folders the current path template produces."
        }
        Tab::Clean => {
            "Rows whose file is gone are forgotten; an asset left with no files loses its sidecar and is exported again next run. Sidecars without an asset are deleted. Dry run only lists."
        }
        Tab::Dates => {
            "Matches folder, name or original filename. Pick a date per asset, then Apply stores it, rewrites metadata and moves the files."
        }
    }
}

pub fn run_label(tab: Tab, args: &RunArgs) -> &'static str {
    match tab {
        Tab::Consolidate => "Consolidate",
        Tab::Clean if args.dry => "List",
        Tab::Clean => "Remove",
        Tab::Dates if args.arg.trim().is_empty() => "Scan",
        Tab::Dates => "Find",
        Tab::ApplePhotos | Tab::OneDrive | Tab::Dropbox | Tab::GoogleTakeout => "Run",
    }
}

pub fn confirmation_message(c: &Confirmation) -> &'static str {
    match c {
        Confirmation::CleanDeletes {
            unreferenced: false,
        } => "Delete sidecars whose files are gone and forget their rows. This cannot be undone.",
        Confirmation::CleanDeletes { unreferenced: true } => {
            "Delete sidecars whose files are gone, forget their rows, and delete every media file the state does not reference. This cannot be undone."
        }
    }
}

pub fn blocker(tab: Tab, b: &Blocker) -> String {
    match b {
        Blocker::Running { tab } => format!("{} is running.", tab_name(*tab)),
        Blocker::NoExportFolder => "Choose a folder to export to first.".to_owned(),
        Blocker::ArgRequired => format!("Choose a {} first.", arg_label(tab).to_lowercase()),
        Blocker::DropboxKeyMissing { config_path } => {
            format!("Set dropbox_app_key in {config_path} to your Dropbox app's key.")
        }
        Blocker::Unavailable => "Apple Photos is only available on macOS".to_owned(),
    }
}

pub fn status(s: &Status) -> String {
    match s {
        Status::Idle => "Idle".to_owned(),
        Status::Running { tab } => format!("Running {}…", tab_name(*tab)),
        Status::CheckingDates => "Checking dates…".to_owned(),
        Status::ApplyingDates => "Applying dates…".to_owned(),
        Status::Done { errors: 0 } => "Done".to_owned(),
        Status::Done { errors } => format!("Done with {errors} errors"),
        Status::Failed => "Failed".to_owned(),
    }
}

pub fn status_is_error(s: &Status) -> bool {
    matches!(s, Status::Done { errors } if *errors > 0) || *s == Status::Failed
}

pub fn stat(kind: StatKind) -> &'static str {
    match kind {
        StatKind::Exported => "exported",
        StatKind::Planned => "planned",
        StatKind::Duplicates => "duplicates",
        StatKind::Skipped => "skipped",
        StatKind::Filtered => "filtered",
        StatKind::Errors => "errors",
        StatKind::Moved => "moved",
        StatKind::WouldMove => "would move",
        StatKind::Renamed => "renamed",
        StatKind::Unchanged => "unchanged",
        StatKind::NoMetadata => "no metadata",
        StatKind::RowsChecked => "rows checked",
        StatKind::DeadRows => "dead rows",
        StatKind::AssetsReset => "assets reset",
        StatKind::SidecarsRemoved => "sidecars removed",
        StatKind::SidecarsToRemove => "sidecars to remove",
        StatKind::UnreferencedFiles => "unreferenced files",
        StatKind::FilesRemoved => "files removed",
        StatKind::FilesToRemove => "files to remove",
    }
}

pub fn chip(state: RowState) -> &'static str {
    match state {
        RowState::Exported => "Exported",
        RowState::Moved => "Moved",
        RowState::Redated => "Redated",
        RowState::Duplicate => "Duplicate",
        RowState::Metadata => "Metadata only",
        RowState::Unchanged => "Nothing new",
        RowState::Missing => "Missing file",
        RowState::Abandoned => "Orphan sidecar",
        RowState::Unclaimed => "Stray sidecar",
        RowState::Unreferenced => "Unreferenced",
        RowState::Failed => "Failed",
        RowState::Other => "Other",
    }
}

pub fn clean_reason(r: CleanReason) -> &'static str {
    match r {
        CleanReason::Missing => "missing file",
        CleanReason::Abandoned => "orphan sidecar",
        CleanReason::Unclaimed => "stray sidecar",
        CleanReason::Unreferenced => "unreferenced",
    }
}

pub fn clean_outcome(o: CleanOutcome) -> &'static str {
    match o {
        CleanOutcome::Removed => "removed",
        CleanOutcome::Kept => "kept",
        CleanOutcome::WouldRemove => "would remove",
    }
}

pub fn run_title(title: Option<&RunTitle>) -> String {
    let Some(title) = title else {
        return String::new();
    };
    match title {
        RunTitle::Redate { count } => format!("redate {count} assets"),
        RunTitle::Run {
            tab,
            arg,
            dry,
            option,
            template,
        } => {
            let tag = if *dry { " (dry run)" } else { "" };
            match tab {
                Tab::Consolidate => format!("consolidate {template}{tag}"),
                Tab::Clean if *option => format!("clean + unreferenced files{tag}"),
                Tab::Clean => format!("clean{tag}"),
                Tab::Dates if arg.is_empty() => format!("scan dates{tag}"),
                Tab::Dates => format!("find {arg}{tag}"),
                Tab::OneDrive if arg.is_empty() => format!("onedrive (whole drive){tag}"),
                Tab::OneDrive => format!("onedrive {arg}{tag}"),
                Tab::Dropbox if arg.is_empty() => format!("dropbox (everything){tag}"),
                Tab::Dropbox => format!("dropbox {arg}{tag}"),
                Tab::ApplePhotos => format!("apple-photos {arg}{tag}"),
                Tab::GoogleTakeout => format!("google-takeout {arg}{tag}"),
            }
        }
    }
}

/// A run-header fact as (value, label, tooltip); value is empty for facts without a number.
pub fn fact(f: &Fact) -> (String, String, String) {
    match f {
        Fact::Found { total, place, name } => (
            total.to_string(),
            format!("assets in {name}"),
            place.clone(),
        ),
        Fact::VisibleToPhotoKit { total } => (
            total.to_string(),
            "visible to PhotoKit".to_owned(),
            String::new(),
        ),
        Fact::Changed { total } => (
            total.to_string(),
            "new or changed".to_owned(),
            String::new(),
        ),
        Fact::Inventory {
            files,
            sidecars,
            dir,
        } => (
            files.to_string(),
            format!("files, {sidecars} sidecars"),
            dir.clone(),
        ),
        Fact::Warning { text } => (String::new(), text.clone(), String::new()),
        Fact::Listing { path, name } => (String::new(), format!("listing {name}"), path.clone()),
        Fact::Reading { path, name } => (String::new(), format!("reading {name}"), path.clone()),
    }
}

pub fn sign_in(prompt: &SignIn) -> &'static str {
    match prompt {
        SignIn::Browser => "Approve access in your browser",
        SignIn::DeviceCode { .. } => "Sign in with code",
    }
}

pub fn subject_message(subject: Option<&str>, message: &str) -> String {
    match subject {
        Some(s) if !s.is_empty() => format!("{s}: {message}"),
        _ => message.to_owned(),
    }
}

pub fn date_key(key: DateKey) -> &'static str {
    match key {
        DateKey::Create => "Create",
        DateKey::Original => "Original",
        DateKey::Modify => "Modify",
    }
}

/// A wall-clock timestamp (Unix seconds of the wall clock read as UTC) in the locale's format.
pub fn wall_clock(at: i64) -> String {
    glib::DateTime::from_unix_utc(at)
        .and_then(|d| d.format("%x %X"))
        .map(String::from)
        .unwrap_or_default()
}

pub fn current_date(at: Option<i64>) -> String {
    at.map_or_else(|| "no date".to_owned(), wall_clock)
}

pub fn dates_summary(
    scanning: bool,
    scan: Option<(u64, u64)>,
    rows: u32,
    query: bool,
    chosen: u32,
) -> String {
    let prefix = match scan {
        Some((checked, total)) if scanning => format!("checked {checked}/{total}, "),
        _ => String::new(),
    };
    let word = if query { "matching" } else { "flagged" };
    format!("{prefix}{rows} {word}, {chosen} chosen")
}

pub fn dates_empty(scanning: bool, query: bool) -> &'static str {
    if scanning {
        "Checking…"
    } else if query {
        "Nothing matches."
    } else {
        "Every asset's folder date matches its file."
    }
}

pub fn dates_apply(applying: bool, chosen: u32) -> String {
    if applying {
        "Applying…".to_owned()
    } else {
        format!("Apply {chosen}")
    }
}

pub fn date_source(d: DateSource) -> &'static str {
    match d {
        DateSource::Source => "Source metadata first",
        DateSource::File => "File EXIF first",
    }
}

pub fn zone(z: ZoneMode) -> &'static str {
    match z {
        ZoneMode::Asset => "As recorded",
        ZoneMode::Local if cfg!(target_os = "macos") => "This Mac's zone",
        ZoneMode::Local => "This computer's zone",
        ZoneMode::Utc => "UTC",
        ZoneMode::Fixed => "Fixed offset",
    }
}

pub fn media(m: Media) -> &'static str {
    match m {
        Media::All => "All",
        Media::Photos => "Photos",
        Media::Videos => "Videos",
    }
}

pub fn metadata(m: Metadata) -> &'static str {
    match m {
        Metadata::Both => "In files + sidecar",
        Metadata::Embed => "In files",
        Metadata::Sidecar => "Sidecar only",
    }
}

pub fn field_group(g: FieldGroup) -> &'static str {
    match g {
        FieldGroup::Date => "Date",
        FieldGroup::Place => "Place",
        FieldGroup::Tags => "Tags",
        FieldGroup::Camera => "Camera",
        FieldGroup::Other => "Other",
    }
}

pub fn field_help(name: &str) -> &'static str {
    match name {
        "take.year" => "year taken, 2019",
        "take.month" => "month taken, 05",
        "take.day" => "day taken, 03",
        "take.date" => "date taken, 2019-05-03",
        "place.name" => "place named by the source; {place.name|place.city} falls back",
        "place.city" => "city",
        "place.state" => "state or province",
        "place.country" => "country code, FR",
        "keyword" => "keyword n",
        "album" => "album n",
        "label" => "machine label n",
        "people" => "person n",
        "camera.make" => "Apple",
        "camera.model" => "iPhone 15 Pro",
        "camera.lens" => "lens model",
        "title" => "title",
        "type" => "photo or video",
        "stem" => "filename without extension",
        "uuid" => "asset id",
        _ => "",
    }
}

pub fn layout_summary(s: &Settings) -> String {
    let path = s.path.trim();
    let file = s.filename.trim();
    format!(
        "{}/{}",
        if path.is_empty() { "…" } else { path },
        if file.is_empty() { "{stem}" } else { file }
    )
}

pub fn dates_settings_summary(s: &Settings) -> String {
    let source = match s.date_source {
        DateSource::Source => "source metadata first",
        DateSource::File => "file EXIF first",
    };
    let zone = match s.zone {
        ZoneMode::Asset => "as recorded".to_owned(),
        ZoneMode::Local if cfg!(target_os = "macos") => "this Mac's zone".to_owned(),
        ZoneMode::Local => "this computer's zone".to_owned(),
        ZoneMode::Utc => "UTC".to_owned(),
        ZoneMode::Fixed => {
            let offset = s.offset.trim();
            format!("UTC{}", if offset.is_empty() { "+00:00" } else { offset })
        }
    };
    format!("{source} · {zone}")
}

fn list(text: &str) -> String {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn filter_parts(s: &Settings) -> Vec<String> {
    let mut parts = Vec::new();
    match s.media {
        Media::All => {}
        Media::Photos => parts.push("photos".to_owned()),
        Media::Videos => parts.push("videos".to_owned()),
    }
    if s.favorites {
        parts.push("favourites".to_owned());
    }
    let (from, to) = (s.from.trim(), s.to.trim());
    if !from.is_empty() || !to.is_empty() {
        let end = |d: &str| {
            if d.is_empty() {
                "…".to_owned()
            } else {
                d.to_owned()
            }
        };
        parts.push(format!("{} to {}", end(from), end(to)));
    }
    let albums = list(&s.albums);
    if !albums.is_empty() {
        parts.push(format!("albums: {albums}"));
    }
    let keywords = list(&s.keywords);
    if !keywords.is_empty() {
        parts.push(format!("keywords: {keywords}"));
    }
    parts
}

pub fn filter_summary(s: &Settings) -> String {
    let parts = filter_parts(s);
    if parts.is_empty() {
        "everything".to_owned()
    } else {
        parts.join(" · ")
    }
}

pub fn filter_hint(s: &Settings) -> &'static str {
    if filter_parts(s).is_empty() {
        "Everything is exported."
    } else {
        "Only matching assets are exported. Consolidate and Dates ignore the filter."
    }
}

pub fn files_summary(s: &Settings) -> String {
    let edits = if s.edits {
        "originals + edits"
    } else {
        "originals"
    };
    let place = match s.metadata {
        Metadata::Both => "in files + sidecar",
        Metadata::Embed => "in files",
        Metadata::Sidecar => "in sidecar",
    };
    format!("{edits} · metadata {place}")
}

pub fn preview_root(root: Option<&str>) -> &str {
    root.unwrap_or("Export")
}

pub fn preview_dir(dir: &str) -> &str {
    if dir.is_empty() { "…" } else { dir }
}

pub fn preview_sample(stem: Option<&str>) -> String {
    match stem {
        Some(stem) => format!("from {stem}"),
        None => "sample photo until something is exported".to_owned(),
    }
}

pub fn preview_problem(unknown: &[String]) -> Option<String> {
    match unknown {
        [] => None,
        [name] => Some(format!(
            "Unknown field: {name}. Pick from the list; it won't be saved until it's fixed."
        )),
        names => Some(format!(
            "Unknown fields: {}. Pick from the list; it won't be saved until it's fixed.",
            names.join(", ")
        )),
    }
}

pub fn settings_problem(p: &SettingsProblem) -> String {
    match p {
        SettingsProblem::FromDate { value } => format!("from date {value:?} is not YYYY-MM-DD"),
        SettingsProblem::ToDate { value } => format!("to date {value:?} is not YYYY-MM-DD"),
        SettingsProblem::Offset { value } => {
            format!("date_zone {value:?} is not asset, local, utc or ±HH:MM")
        }
        SettingsProblem::UnknownFields { template, names } => {
            let what = match template {
                TemplateKind::Folder => "path",
                TemplateKind::Filename => "filename",
            };
            let noun = if names.len() == 1 { "field" } else { "fields" };
            format!("{what} uses unknown {noun} {}", names.join(", "))
        }
        SettingsProblem::FromAfterTo => "filter.from is after filter.to".to_owned(),
        SettingsProblem::CheckInterval { .. } => {
            "Check interval must be between 1 and 1440 minutes.".to_owned()
        }
    }
}

pub fn error(tab: Tab, e: &CoreError) -> String {
    match e {
        CoreError::Blocked { blocker: b } => blocker(tab, b),
        CoreError::Running { tab } => format!("{} is running.", tab_name(*tab)),
        CoreError::NothingChosen => "Nothing chosen.".to_owned(),
        CoreError::InvalidSettings { problem } => settings_problem(problem),
        CoreError::Failed { detail } => detail.clone(),
    }
}

pub fn settings_error(e: &CoreError) -> String {
    match e {
        CoreError::InvalidSettings { problem } => settings_problem(problem),
        CoreError::Failed { detail } => detail.clone(),
        CoreError::Blocked { .. } | CoreError::Running { .. } | CoreError::NothingChosen => {
            error(Tab::Consolidate, e)
        }
    }
}

pub const NOTHING_NEW: &str = "nothing new";
