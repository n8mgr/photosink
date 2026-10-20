const DEFAULT_LIBRARY: &str = "~/Pictures/Photos Library.photoslibrary";

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

#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct RunArgs {
    pub arg: String,
    pub dry: bool,
    pub option: bool,
}

/// A run that needs the user's go-ahead first.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Confirmation {
    /// A Clean that deletes; `unreferenced` when it also deletes files the
    /// state doesn't reference.
    CleanDeletes { unreferenced: bool },
}

/// Why a run can't start yet.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Blocker {
    Running {
        tab: Tab,
    },
    NoExportFolder,
    /// The tab's argument is required and blank.
    ArgRequired,
    /// `dropbox_app_key` is not set in the config file at `config_path`.
    DropboxKeyMissing {
        config_path: String,
    },
    /// Apple Photos on a platform other than macOS.
    Unavailable,
}

/// What a run header describes.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum RunTitle {
    /// A run of `tab`. `arg` is trimmed; `dry` is only set on tabs that
    /// offer a dry run; `template` is the folder template, set on Consolidate.
    Run {
        tab: Tab,
        arg: String,
        dry: bool,
        option: bool,
        template: String,
    },
    /// Applying `count` chosen dates.
    Redate { count: u32 },
}

impl Tab {
    /// The run log's file name prefix.
    pub(crate) fn slug(self) -> &'static str {
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

    fn dry_run(self) -> bool {
        self != Tab::Dates
    }

    pub(crate) fn default_args(self) -> RunArgs {
        RunArgs {
            arg: match self {
                Tab::ApplePhotos => DEFAULT_LIBRARY.to_owned(),
                _ => String::new(),
            },
            dry: false,
            option: false,
        }
    }

    pub(crate) fn confirmation(self, args: &RunArgs) -> Option<Confirmation> {
        (self == Tab::Clean && !args.dry).then_some(Confirmation::CleanDeletes {
            unreferenced: args.option,
        })
    }

    pub(crate) fn blocker(self, args: &RunArgs, export_dir: &str) -> Option<Blocker> {
        if self == Tab::ApplePhotos && !cfg!(target_os = "macos") {
            return Some(Blocker::Unavailable);
        }
        if export_dir.trim().is_empty() {
            return Some(Blocker::NoExportFolder);
        }
        match self {
            Tab::GoogleTakeout | Tab::ApplePhotos if args.arg.trim().is_empty() => {
                Some(Blocker::ArgRequired)
            }
            _ => None,
        }
    }

    pub(crate) fn run_title(self, args: &RunArgs, template: &str) -> RunTitle {
        RunTitle::Run {
            tab: self,
            arg: args.arg.trim().to_owned(),
            dry: args.dry && self.dry_run(),
            option: args.option,
            template: if self == Tab::Consolidate {
                template.to_owned()
            } else {
                String::new()
            },
        }
    }
}

/// Every tab available on this platform, in sidebar order.
pub(crate) fn tabs() -> Vec<Tab> {
    let mut out = Vec::new();
    if cfg!(target_os = "macos") {
        out.push(Tab::ApplePhotos);
    }
    out.extend([
        Tab::OneDrive,
        Tab::Dropbox,
        Tab::GoogleTakeout,
        Tab::Consolidate,
        Tab::Clean,
        Tab::Dates,
    ]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dry() -> RunArgs {
        RunArgs {
            dry: true,
            ..RunArgs::default()
        }
    }

    #[test]
    fn titles_follow_args() {
        let q = RunArgs {
            arg: " PR5A ".into(),
            dry: true,
            ..RunArgs::default()
        };
        assert_eq!(
            Tab::Dates.run_title(&q, "{take.year}"),
            RunTitle::Run {
                tab: Tab::Dates,
                arg: "PR5A".into(),
                dry: false,
                option: false,
                template: String::new(),
            }
        );
        assert_eq!(
            Tab::Consolidate.run_title(&dry(), "{take.year}"),
            RunTitle::Run {
                tab: Tab::Consolidate,
                arg: String::new(),
                dry: true,
                option: false,
                template: "{take.year}".into(),
            }
        );
    }

    #[test]
    fn only_a_real_clean_asks_first() {
        let all = RunArgs {
            option: true,
            ..RunArgs::default()
        };
        assert_eq!(
            Tab::Clean.confirmation(&all),
            Some(Confirmation::CleanDeletes { unreferenced: true })
        );
        assert!(Tab::Clean.confirmation(&dry()).is_none());
        assert!(Tab::Consolidate.confirmation(&RunArgs::default()).is_none());
    }

    #[test]
    fn blocks_without_an_export_folder_or_required_arg() {
        assert_eq!(
            Tab::GoogleTakeout.blocker(&RunArgs::default(), "/x"),
            Some(Blocker::ArgRequired)
        );
        assert_eq!(
            Tab::Consolidate.blocker(&RunArgs::default(), ""),
            Some(Blocker::NoExportFolder)
        );
        assert_eq!(Tab::Consolidate.blocker(&RunArgs::default(), "/x"), None);
    }

    #[test]
    fn apple_photos_only_on_macos() {
        assert_eq!(
            tabs().contains(&Tab::ApplePhotos),
            cfg!(target_os = "macos")
        );
    }
}
