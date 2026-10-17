use photosink::DEFAULT_LIBRARY;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Tab {
    ApplePhotos,
    OneDrive,
    Dropbox,
    GoogleTakeout,
    Consolidate,
    Clean,
    Dates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PickerKind {
    /// Free text, no picker.
    None,
    /// A Photos library package.
    Library,
    /// A folder, or an archive file.
    FolderOrArchive,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ArgSpec {
    pub label: String,
    pub placeholder: String,
    pub picker: PickerKind,
    /// File extensions the picker offers, without dots.
    pub extensions: Vec<String>,
    /// Pressing return in the field starts the run.
    pub run_on_enter: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TabInfo {
    pub tab: Tab,
    pub name: String,
    pub arg: Option<ArgSpec>,
    /// A labelled switch shown under the argument, stored as `RunArgs::option`.
    pub option: Option<String>,
    pub hint: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Group {
    pub name: String,
    pub tabs: Vec<TabInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct RunArgs {
    pub arg: String,
    pub dry: bool,
    pub option: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Confirmation {
    pub title: String,
    pub message: String,
    pub ok: String,
}

impl Tab {
    pub fn name(self) -> &'static str {
        match self {
            Tab::ApplePhotos => "Apple Photos",
            Tab::OneDrive => "OneDrive",
            Tab::Dropbox => "Dropbox",
            Tab::GoogleTakeout => "Google Takeout",
            Tab::Consolidate => "Consolidate",
            Tab::Clean => "Clean",
            Tab::Dates => "Dates",
        }
    }

    /// The run log's file name prefix and the run header's verb.
    pub fn slug(self) -> &'static str {
        match self {
            Tab::ApplePhotos => "apple-photos",
            Tab::OneDrive => "onedrive",
            Tab::Dropbox => "dropbox",
            Tab::GoogleTakeout => "google-takeout",
            Tab::Consolidate => "consolidate",
            Tab::Clean => "clean",
            Tab::Dates => "dates",
        }
    }

    pub fn all() -> Vec<Tab> {
        groups()
            .into_iter()
            .flat_map(|g| g.tabs)
            .map(|t| t.tab)
            .collect()
    }

    pub fn default_args(self) -> RunArgs {
        RunArgs {
            arg: match self {
                Tab::ApplePhotos => DEFAULT_LIBRARY.to_owned(),
                _ => String::new(),
            },
            dry: false,
            option: false,
        }
    }

    pub fn info(self) -> TabInfo {
        let arg = |label: &str, placeholder: &str, picker, extensions: &[&str], run_on_enter| {
            Some(ArgSpec {
                label: label.into(),
                placeholder: placeholder.into(),
                picker,
                extensions: extensions.iter().map(|s| s.to_string()).collect(),
                run_on_enter,
            })
        };
        let (arg, option, hint, dry_run) = match self {
            Tab::ApplePhotos => (
                arg(
                    "Library",
                    "Photos library",
                    PickerKind::Library,
                    &["photoslibrary"],
                    false,
                ),
                None,
                "Exports originals and metadata from the Photos library through PhotoKit.",
                true,
            ),
            Tab::OneDrive => (
                arg(
                    "Folder",
                    "Folder under the drive root; empty for the whole drive",
                    PickerKind::None,
                    &[],
                    false,
                ),
                None,
                "Signs in with a device code the first time, then exports what changed since the last run.",
                true,
            ),
            Tab::Dropbox => (
                arg(
                    "Folder",
                    "Folder in your Dropbox; empty for everything",
                    PickerKind::None,
                    &[],
                    false,
                ),
                None,
                "Signs in through the browser the first time, then exports what changed since the last run.",
                true,
            ),
            Tab::GoogleTakeout => (
                arg(
                    "Takeout",
                    "Folder of archives, a single archive, or an extracted tree",
                    PickerKind::FolderOrArchive,
                    &["zip", "tgz", "gz"],
                    false,
                ),
                None,
                "Reads the archives in place; nothing needs extracting first.",
                true,
            ),
            Tab::Consolidate => (
                None,
                None,
                "Moves everything already exported into the folders the current path template produces.",
                true,
            ),
            Tab::Clean => (
                None,
                Some("Also delete files the state doesn't reference".into()),
                "Rows whose file is gone are forgotten; an asset left with no files loses its sidecar and is exported again next run. Sidecars without an asset are deleted. Dry run only lists.",
                true,
            ),
            Tab::Dates => (
                arg(
                    "Find",
                    "Leave empty to list only assets whose date disagrees with the file",
                    PickerKind::None,
                    &[],
                    true,
                ),
                None,
                "Matches folder, name or original filename. Pick a date per asset, then Apply stores it, rewrites metadata and moves the files.",
                false,
            ),
        };
        TabInfo {
            tab: self,
            name: self.name().into(),
            arg,
            option,
            hint: hint.into(),
            dry_run,
        }
    }

    pub fn run_label(self, args: &RunArgs) -> String {
        match self {
            Tab::Consolidate => "Consolidate",
            Tab::Clean if args.dry => "List",
            Tab::Clean => "Remove",
            Tab::Dates if args.arg.trim().is_empty() => "Scan",
            Tab::Dates => "Find",
            _ => "Run",
        }
        .into()
    }

    /// The question to ask before a run that deletes things.
    pub fn confirmation(self, args: &RunArgs) -> Option<Confirmation> {
        if self != Tab::Clean || args.dry {
            return None;
        }
        Some(Confirmation {
            title: "Clean the export".into(),
            message: if args.option {
                "Delete sidecars whose files are gone, forget their rows, and delete every media file the state does not reference. This cannot be undone."
            } else {
                "Delete sidecars whose files are gone and forget their rows. This cannot be undone."
            }
            .into(),
            ok: "Delete".into(),
        })
    }

    /// Why the run can't start yet, if it can't.
    pub fn blocker(self, args: &RunArgs, export_dir: &str) -> Option<String> {
        if export_dir.trim().is_empty() {
            return Some("Choose a folder to export to first.".into());
        }
        match self {
            Tab::GoogleTakeout | Tab::ApplePhotos if args.arg.trim().is_empty() => Some(format!(
                "Choose a {} first.",
                self.info()
                    .arg
                    .map(|a| a.label.to_lowercase())
                    .unwrap_or_default()
            )),
            _ => None,
        }
    }

    /// The run header shown above the rows of a run.
    pub fn run_title(self, args: &RunArgs, template: &str) -> String {
        let tag = if args.dry && self.info().dry_run {
            " (dry run)"
        } else {
            ""
        };
        match self {
            Tab::Consolidate => format!("consolidate {template}{tag}"),
            Tab::Clean => format!(
                "clean{}{tag}",
                if args.option {
                    " + unreferenced files"
                } else {
                    ""
                }
            ),
            Tab::Dates => match args.arg.trim() {
                "" => "scan dates".into(),
                q => format!("find {q}"),
            },
            Tab::OneDrive if args.arg.trim().is_empty() => format!("onedrive (whole drive){tag}"),
            Tab::Dropbox if args.arg.trim().is_empty() => format!("dropbox (everything){tag}"),
            _ => format!("{} {}{tag}", self.slug(), args.arg.trim()),
        }
    }
}

pub fn groups() -> Vec<Group> {
    let mut import = Vec::new();
    if cfg!(target_os = "macos") {
        import.push(Tab::ApplePhotos);
    }
    import.extend([Tab::OneDrive, Tab::Dropbox, Tab::GoogleTakeout]);
    vec![
        Group {
            name: "Import".into(),
            tabs: import.into_iter().map(Tab::info).collect(),
        },
        Group {
            name: "Maintain".into(),
            tabs: [Tab::Consolidate, Tab::Clean, Tab::Dates]
                .into_iter()
                .map(Tab::info)
                .collect(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_args() {
        let dry = RunArgs {
            dry: true,
            ..RunArgs::default()
        };
        assert_eq!(Tab::Clean.run_label(&dry), "List");
        assert_eq!(Tab::Clean.run_label(&RunArgs::default()), "Remove");
        let q = RunArgs {
            arg: " PR5A ".into(),
            ..RunArgs::default()
        };
        assert_eq!(Tab::Dates.run_label(&q), "Find");
        assert_eq!(Tab::Dates.run_title(&q, ""), "find PR5A");
        assert!(Tab::Clean.confirmation(&RunArgs::default()).is_some());
        assert!(Tab::Clean.confirmation(&dry).is_none());
        assert_eq!(
            Tab::Consolidate.run_title(&dry, "{take.year}"),
            "consolidate {take.year} (dry run)"
        );
        assert!(
            Tab::GoogleTakeout
                .blocker(&RunArgs::default(), "/x")
                .is_some()
        );
        assert!(Tab::Consolidate.blocker(&RunArgs::default(), "").is_some());
    }

    #[test]
    fn apple_photos_only_on_macos() {
        assert_eq!(
            Tab::all().contains(&Tab::ApplePhotos),
            cfg!(target_os = "macos")
        );
    }
}
