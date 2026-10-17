pub mod clean;
pub mod config;
pub mod consolidate;
pub mod export;
pub mod photo;
pub mod redate;
pub mod sources;
pub mod state;
pub mod template;

use anyhow::{Context, Result};
use config::Config;
use directories::ProjectDirs;
use export::Summary;
#[cfg(target_os = "macos")]
use sources::apple_photos;
use state::State;
use std::path::{Path, PathBuf};

/// One step of a run, sent to the window as structured data and rendered to
/// text for the on-disk run log.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Log {
        path: String,
    },
    Found {
        total: usize,
        place: String,
    },
    Visible {
        total: usize,
    },
    Changed {
        total: usize,
    },
    Listing {
        path: String,
    },
    Reading {
        path: String,
    },
    SignIn {
        url: String,
        code: String,
    },
    /// A browser sign-in that finishes on its own once approved.
    Authorize {
        url: String,
    },
    Inventory {
        files: usize,
        sidecars: usize,
        dir: String,
    },
    Clean {
        path: String,
        reason: clean::Reason,
        removed: bool,
    },
    Checking {
        total: usize,
        query: Option<String>,
    },
    Checked {
        n: usize,
        total: usize,
        flagged: usize,
    },
    Flagged(redate::Candidate),
    Asset {
        n: usize,
        total: usize,
        name: String,
    },
    Placed {
        n: usize,
        total: usize,
        file: String,
        dest: String,
    },
    Duplicate {
        n: usize,
        total: usize,
        file: String,
        existing: String,
    },
    Embedded {
        n: usize,
        total: usize,
        path: String,
    },
    Moved {
        n: usize,
        total: usize,
        stem: String,
        dest: String,
    },
    Redated {
        stem: String,
        from: String,
        to: String,
    },
    Error {
        n: Option<usize>,
        total: Option<usize>,
        subject: String,
        text: String,
    },
}

impl Event {
    pub fn error(subject: impl Into<String>, text: impl std::fmt::Display) -> Event {
        Event::Error {
            n: None,
            total: None,
            subject: subject.into(),
            text: text.to_string(),
        }
    }
}

impl std::fmt::Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Log { path } => write!(f, "log: {path}"),
            Event::Found { total, place } => write!(f, "{total} assets in {place}"),
            Event::Visible { total } => write!(f, "{total} assets visible to PhotoKit"),
            Event::Changed { total } => write!(f, "{total} new or changed assets"),
            Event::Listing { path } => write!(f, "listing {path}"),
            Event::Reading { path } => write!(f, "reading {path}"),
            Event::SignIn { url, code } => write!(f, "sign in at {url} with code {code}"),
            Event::Authorize { url } => write!(f, "approve access at {url}"),
            Event::Inventory {
                files,
                sidecars,
                dir,
            } => write!(f, "{files} files and {sidecars} sidecars under {dir}"),
            Event::Clean {
                path,
                reason,
                removed,
            } => {
                write!(
                    f,
                    "{path}: {reason}{}",
                    if *removed { "; removed" } else { "" }
                )
            }
            Event::Checking {
                total,
                query: Some(q),
            } => write!(f, "finding {q:?} among {total} assets"),
            Event::Checking { total, query: None } => write!(f, "checking dates of {total} assets"),
            Event::Checked { n, total, flagged } => write!(f, "[{n}/{total}] {flagged} flagged"),
            Event::Flagged(c) => write!(
                f,
                "{}/{}: {} (create {}, original {}, modify {})",
                c.dir,
                c.stem,
                c.current.as_deref().unwrap_or("no date"),
                c.create.as_deref().unwrap_or("-"),
                c.original.as_deref().unwrap_or("-"),
                c.modify.as_deref().unwrap_or("-"),
            ),
            Event::Asset { n, total, name } => write!(f, "[{n}/{total}] {name}"),
            Event::Placed {
                n,
                total,
                file,
                dest,
            } => write!(f, "[{n}/{total}]   {file} -> {dest}"),
            Event::Duplicate {
                n,
                total,
                file,
                existing,
            } => write!(f, "[{n}/{total}]   {file} == {existing}"),
            Event::Embedded { n, total, path } => write!(f, "[{n}/{total}]   {path} <- xmp"),
            Event::Moved {
                n,
                total,
                stem,
                dest,
            } => write!(f, "[{n}/{total}] {stem} -> {dest}"),
            Event::Redated { stem, from, to } => write!(f, "{stem}: {from} -> {to}"),
            Event::Error {
                n: Some(n),
                total: Some(total),
                subject,
                text,
            } => write!(f, "[{n}/{total}] {subject}: {text}"),
            Event::Error { subject, text, .. } => write!(f, "{subject}: {text}"),
        }
    }
}

pub type Progress<'a> = &'a (dyn Fn(Event) + Sync);

pub const DEFAULT_LIBRARY: &str = "~/Pictures/Photos Library.photoslibrary";

#[derive(Debug, Clone, serde::Serialize)]
pub struct PathPreview {
    pub dir: String,
    pub file: String,
    pub ext: String,
    pub unknown_path: Vec<String>,
    pub unknown_filename: Vec<String>,
    pub sample: String,
    pub fields: Vec<photo::FieldInfo>,
}

/// Renders the folder and filename templates against `sample`, reporting
/// unknown fields in each.
pub fn preview_path(
    cfg: Option<&Config>,
    path: &str,
    filename: &str,
    sample: &photo::Asset,
    label: &str,
) -> PathPreview {
    let zoned = match cfg {
        Some(cfg) => cfg.zoned(sample),
        None => sample.clone(),
    };
    let file = {
        let s = template::render(filename, |k, i| zoned.field(k, i)).replace('/', "-");
        if s.is_empty() {
            template::sanitize(zoned.stem())
        } else {
            s
        }
    };
    PathPreview {
        dir: template::render(path, |k, i| zoned.field(k, i)),
        file,
        ext: Path::new(&sample.filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("HEIC")
            .to_owned(),
        unknown_path: template::unknown(path, &photo::field_names()),
        unknown_filename: template::unknown(filename, &photo::field_names()),
        sample: label.to_owned(),
        fields: photo::FIELDS.to_vec(),
    }
}

/// A progress sink that also appends every line to a file under the data
/// directory, so nothing the window drops is lost.
pub struct RunLog {
    pub path: PathBuf,
    file: std::sync::Mutex<std::fs::File>,
}

impl RunLog {
    pub fn write(&self, line: &str) {
        use std::io::Write;
        if let Ok(mut f) = self.file.lock() {
            let _ = writeln!(f, "{line}");
        }
    }
}

pub struct App {
    pub cfg: Config,
    pub state: State,
    dirs: ProjectDirs,
}

pub fn config_path(custom: Option<&Path>) -> Result<PathBuf> {
    Ok(match custom {
        Some(p) => p.to_path_buf(),
        None => config::project_dirs()?.config_dir().join("config.toml"),
    })
}

impl App {
    pub fn open(config: Option<&Path>, state: Option<&Path>) -> Result<App> {
        let dirs = config::project_dirs()?;
        let config_path = config_path(config)?;
        let cfg = Config::load(&config_path)
            .with_context(|| format!("load config {}", config_path.display()))?;
        let state = match state {
            Some(p) => State::open(p)?,
            None => State::open(&dirs.data_dir().join("state.sqlite"))?,
        };
        Ok(App { cfg, state, dirs })
    }

    /// Opens `logs/<name>-<timestamp>.log` in the data directory.
    pub fn run_log(&self, name: &str) -> Result<RunLog> {
        let dir = self.dirs.data_dir().join("logs");
        std::fs::create_dir_all(&dir)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = dir.join(format!("{name}-{stamp}.log"));
        let file = std::fs::File::create(&path)?;
        Ok(RunLog {
            path,
            file: std::sync::Mutex::new(file),
        })
    }

    #[cfg(target_os = "macos")]
    pub fn apple_photos(&self, library: &Path, progress: Progress, dry: bool) -> Result<Summary> {
        let library = config::expand_home(library);
        let assets = apple_photos::library::load(&library)?;
        progress(Event::Found {
            total: assets.len(),
            place: library.display().to_string(),
        });
        apple_photos::photokit::authorize()?;
        let ids = apple_photos::photokit::list_assets();
        progress(Event::Visible { total: ids.len() });
        Ok(export::run(
            &apple_photos::PhotoKit,
            &self.cfg,
            &self.state,
            &assets,
            &ids,
            progress,
            dry,
        ))
    }

    pub fn onedrive(&self, folder: &str, progress: Progress, dry: bool) -> Result<Summary> {
        let token_path = self.dirs.data_dir().join("onedrive_token.json");
        let key = format!("onedrive_delta:{folder}");
        let src = sources::onedrive::OneDrive::connect(
            &self.cfg.onedrive_client_id,
            &token_path,
            progress,
        )?;
        let (assets, next) = src.list(folder, self.state.get(&key)?)?;
        progress(Event::Changed {
            total: assets.len(),
        });
        let ids: Vec<_> = assets.keys().map(|k| (k.clone(), k.clone())).collect();
        let s = export::run(&src, &self.cfg, &self.state, &assets, &ids, progress, dry);
        if s.errors == 0 && !dry {
            self.state.set(&key, &next)?;
        }
        Ok(s)
    }

    pub fn dropbox(&self, folder: &str, progress: Progress, dry: bool) -> Result<Summary> {
        let token_path = self.dirs.data_dir().join("dropbox_token.json");
        let key = format!("dropbox_cursor:{}", folder.trim().trim_matches('/'));
        let src = sources::dropbox::Dropbox::connect(
            self.cfg.dropbox_app_key.as_deref().unwrap_or(""),
            &token_path,
            progress,
        )?;
        let (assets, next) = src.list(folder, self.state.get(&key)?)?;
        progress(Event::Changed {
            total: assets.len(),
        });
        let ids: Vec<_> = assets.keys().map(|k| (k.clone(), k.clone())).collect();
        let s = export::run(&src, &self.cfg, &self.state, &assets, &ids, progress, dry);
        if s.errors == 0 && !dry {
            self.state.set(&key, &next)?;
        }
        Ok(s)
    }

    pub fn takeout(&self, path: &Path, progress: Progress, dry: bool) -> Result<Summary> {
        sources::takeout::import(path, &self.cfg, &self.state, progress, dry)
    }

    pub fn consolidate(&self, progress: Progress, dry: bool) -> Result<consolidate::Consolidated> {
        consolidate::run(&self.cfg, &self.state, progress, dry)
    }

    /// Previews the templates against the first exported asset, or a built-in sample.
    pub fn preview_path(&self, path: &str, filename: &str) -> Result<PathPreview> {
        let exported = self
            .state
            .assets_under(&self.cfg.export_dir.to_string_lossy())?
            .into_iter()
            .find_map(|(uuid, dir, stem)| {
                export::stored_asset(&self.state, &dir, &stem, &uuid)
                    .ok()
                    .flatten()
                    .map(|a| (a, stem))
            });
        Ok(match exported {
            Some((asset, stem)) => preview_path(Some(&self.cfg), path, filename, &asset, &stem),
            None => preview_path(
                Some(&self.cfg),
                path,
                filename,
                &photo::Asset::sample(),
                "sample",
            ),
        })
    }

    pub fn clean(&self, orphans: bool, dry: bool, progress: Progress) -> Result<clean::Cleaned> {
        clean::run(&self.cfg, &self.state, orphans, dry, progress)
    }

    pub fn redate_scan(
        &self,
        query: Option<&str>,
        progress: Progress,
    ) -> Result<Vec<redate::Candidate>> {
        redate::scan(&self.cfg, &self.state, query, progress)
    }

    pub fn redate_apply(
        &self,
        choices: &[redate::Choice],
        progress: Progress,
    ) -> Result<consolidate::Consolidated> {
        redate::apply(&self.cfg, &self.state, choices, progress)
    }
}
