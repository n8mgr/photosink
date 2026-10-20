//! The window's model, shared by every platform's UI. A UI renders what
//! `Core` returns, forwards edits back, and re-reads whatever `Listener`
//! reports changed.

pub mod catalog;
pub mod dates;
pub mod grid;
pub mod settings;

use catalog::{Blocker, Confirmation, RunArgs, RunTitle, Tab};
use dates::{DateKey, DateRow, Dates, DatesView};
use grid::{Chip, Grid, Row, RowState};
use photosink::photo::Asset;
use photosink::redate::Choice;
use photosink::{App, Event, config};
use settings::{Field, Preview, Settings, SettingsProblem};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

uniffi::setup_scaffolding!();

const TICK: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// A run can't start; see `Core::blocker`.
    #[error("blocked: {blocker:?}")]
    Blocked { blocker: Blocker },
    /// A run is already going on `tab`.
    #[error("{tab:?} is running")]
    Running { tab: Tab },
    /// `apply_dates` with no date chosen.
    #[error("nothing chosen")]
    NothingChosen,
    #[error("invalid settings: {problem:?}")]
    InvalidSettings { problem: SettingsProblem },
    /// Anything else, with the core's own message.
    #[error("{detail}")]
    Failed { detail: String },
}

impl From<anyhow::Error> for CoreError {
    fn from(e: anyhow::Error) -> Self {
        CoreError::Failed {
            detail: format!("{e:#}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Change {
    /// `status()`, and whether tabs may be switched or runs started.
    Status,
    /// A tab's rows, chips or stats.
    Grid { tab: Tab },
    /// The Dates picker.
    Dates,
}

/// Told what to re-read, at most every 100 ms and never on the caller's
/// thread; UIs hop to their main thread before touching widgets.
#[uniffi::export(with_foreign)]
pub trait Listener: Send + Sync {
    fn changed(&self, change: Change);
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Status {
    Idle,
    /// A run started on `tab`.
    Running {
        tab: Tab,
    },
    /// The scan Dates runs again after applying.
    CheckingDates,
    ApplyingDates,
    /// The last run finished; `errors` rows failed.
    Done {
        errors: u32,
    },
    /// The last run failed outright; its error is the tab's last row.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StatKind {
    Exported,
    /// Exported in a dry run.
    Planned,
    Duplicates,
    Skipped,
    Filtered,
    Errors,
    Moved,
    /// Moved in a dry run.
    WouldMove,
    Renamed,
    Unchanged,
    NoMetadata,
    RowsChecked,
    DeadRows,
    AssetsReset,
    SidecarsRemoved,
    /// Sidecars removed in a dry run.
    SidecarsToRemove,
    UnreferencedFiles,
    FilesRemoved,
    /// Files removed in a dry run.
    FilesToRemove,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Stat {
    pub kind: StatKind,
    pub count: u64,
}

struct Model {
    grids: HashMap<Tab, Grid>,
    stats: HashMap<Tab, Vec<Stat>>,
    args: HashMap<Tab, RunArgs>,
    dates: Dates,
    status: Status,
    running: Option<Tab>,
    settings: Option<Settings>,
    dropbox_key: bool,
    /// The asset templates are previewed against, for the export folder it was read from.
    sample: Option<(String, Option<(Asset, String)>)>,
}

enum Job {
    Run,
    ApplyDates(Vec<Choice>),
}

struct Shared {
    model: Mutex<Model>,
    dirty: Mutex<HashSet<Change>>,
    listener: Arc<dyn Listener>,
}

#[derive(uniffi::Object)]
pub struct Core {
    shared: Arc<Shared>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn stats<T: Into<Vec<(StatKind, usize)>>>(pairs: T) -> Vec<Stat> {
    pairs
        .into()
        .into_iter()
        .map(|(kind, v)| Stat {
            kind,
            count: v as u64,
        })
        .collect()
}

fn config_file() -> anyhow::Result<std::path::PathBuf> {
    let path = photosink::config_path()?;
    if !path.exists() {
        config::write_initial(&path)?;
    }
    Ok(path)
}

impl Shared {
    fn mark(&self, change: Change) {
        lock(&self.dirty).insert(change);
    }

    fn set_status(&self, m: &mut Model, status: Status) {
        m.running = match status {
            Status::Running { tab } => Some(tab),
            Status::CheckingDates | Status::ApplyingDates => Some(Tab::Dates),
            Status::Idle | Status::Done { .. } | Status::Failed => None,
        };
        m.status = status;
        self.mark(Change::Status);
    }

    /// Routes one event from a run on `tab` into the model.
    fn event(&self, tab: Tab, e: Event) {
        let mut m = lock(&self.model);
        match e {
            Event::Checking { total, .. } => {
                m.dates.checked(0, total);
                self.mark(Change::Dates);
            }
            Event::Checked { n, total, .. } => {
                m.dates.checked(n, total);
                self.mark(Change::Dates);
            }
            Event::Flagged(c) => {
                m.dates.flagged(c);
                self.mark(Change::Dates);
            }
            e => {
                m.grids.entry(tab).or_default().apply(&e);
                self.mark(Change::Grid { tab });
            }
        }
    }

    fn run(self: &Arc<Self>, tab: Tab, args: RunArgs, job: Job) {
        let applying = matches!(job, Job::ApplyDates(_));
        let app = match App::open() {
            Ok(app) => app,
            Err(e) => return self.finish(tab, Err(e), applying),
        };
        let log = match app.run_log(if applying { "dates-apply" } else { tab.slug() }) {
            Ok(log) => log,
            Err(e) => return self.finish(tab, Err(e), applying),
        };
        let first = Event::Log {
            path: log.path.display().to_string(),
        };
        log.write(&first.to_string());
        self.event(tab, first);
        let progress = |e: Event| {
            log.write(&e.to_string());
            self.event(tab, e);
        };
        let dry = args.dry;
        let result: anyhow::Result<Vec<Stat>> = match job {
            Job::ApplyDates(choices) => app
                .redate_apply(&choices, &progress)
                .map(|c| consolidated(&c, false)),
            Job::Run => match tab {
                #[cfg(target_os = "macos")]
                Tab::ApplePhotos => app
                    .apple_photos(Path::new(&args.arg), &progress, dry)
                    .map(|s| summary(&s, dry)),
                #[cfg(not(target_os = "macos"))]
                Tab::ApplePhotos => unreachable!("start() returns Blocker::Unavailable"),
                Tab::OneDrive => app
                    .onedrive(args.arg.trim(), &progress, dry)
                    .map(|s| summary(&s, dry)),
                Tab::Dropbox => app
                    .dropbox(args.arg.trim(), &progress, dry)
                    .map(|s| summary(&s, dry)),
                Tab::GoogleTakeout => app
                    .takeout(Path::new(args.arg.trim()), &progress, dry)
                    .map(|s| summary(&s, dry)),
                Tab::Consolidate => app
                    .consolidate(&progress, dry)
                    .map(|c| consolidated(&c, dry)),
                Tab::Clean => app.clean(args.option, &progress, dry).map(|c| {
                    let mut out = vec![
                        (StatKind::RowsChecked, c.rows),
                        (StatKind::DeadRows, c.dead_rows),
                        (StatKind::AssetsReset, c.removed_assets),
                        (
                            if dry {
                                StatKind::SidecarsToRemove
                            } else {
                                StatKind::SidecarsRemoved
                            },
                            c.removed_sidecars,
                        ),
                        (StatKind::UnreferencedFiles, c.orphan_files),
                    ];
                    if args.option {
                        out.push((
                            if dry {
                                StatKind::FilesToRemove
                            } else {
                                StatKind::FilesRemoved
                            },
                            c.removed_files,
                        ));
                    }
                    stats(out)
                }),
                Tab::Dates => {
                    let q = args.arg.trim();
                    app.redate_scan((!q.is_empty()).then_some(q), &progress)
                        .map(|()| Vec::new())
                }
            },
        };
        self.finish(tab, result, applying);
        if applying {
            let failed = lock(&self.model)
                .grids
                .get(&Tab::Dates)
                .map_or(0, Grid::errors)
                > 0;
            if !failed {
                self.rescan(args);
            }
        }
    }

    /// Ends a run on `tab`; `applied` when it applied dates rather than scanning.
    fn finish(&self, tab: Tab, result: anyhow::Result<Vec<Stat>>, applied: bool) {
        let mut m = lock(&self.model);
        m.sample = None;
        if tab == Tab::Dates {
            m.dates.finish();
            self.mark(Change::Dates);
        }
        match result {
            Ok(stats) => {
                if tab != Tab::Dates || applied {
                    m.stats.insert(tab, stats);
                }
                let errors = m.grids.get(&tab).map_or(0, Grid::errors) as u32;
                self.set_status(&mut m, Status::Done { errors });
            }
            Err(e) => {
                m.grids.entry(tab).or_default().push_error(format!("{e:#}"));
                self.set_status(&mut m, Status::Failed);
            }
        }
        self.mark(Change::Grid { tab });
    }

    fn rescan(self: &Arc<Self>, args: RunArgs) {
        {
            let mut m = lock(&self.model);
            let title = Tab::Dates.run_title(&args, "");
            m.grids.entry(Tab::Dates).or_default().start_run(title);
            m.dates.start_scan(!args.arg.trim().is_empty());
            self.set_status(&mut m, Status::CheckingDates);
            self.mark(Change::Grid { tab: Tab::Dates });
            self.mark(Change::Dates);
        }
        self.run(Tab::Dates, args, Job::Run);
    }

    fn spawn_ticker(this: &Arc<Shared>) {
        let weak: Weak<Shared> = Arc::downgrade(this);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(TICK);
                let Some(shared) = weak.upgrade() else { return };
                let changes: Vec<Change> = lock(&shared.dirty).drain().collect();
                for c in changes {
                    shared.listener.changed(c);
                }
            }
        });
    }
}

fn summary(s: &photosink::export::Summary, dry: bool) -> Vec<Stat> {
    stats([
        (
            if dry {
                StatKind::Planned
            } else {
                StatKind::Exported
            },
            s.exported,
        ),
        (StatKind::Duplicates, s.duplicates),
        (StatKind::Skipped, s.skipped),
        (StatKind::Filtered, s.filtered),
        (StatKind::Errors, s.errors),
    ])
}

fn consolidated(c: &photosink::consolidate::Consolidated, dry: bool) -> Vec<Stat> {
    stats([
        (
            if dry {
                StatKind::WouldMove
            } else {
                StatKind::Moved
            },
            c.moved,
        ),
        (StatKind::Renamed, c.restemmed),
        (StatKind::Unchanged, c.unchanged),
        (StatKind::NoMetadata, c.missing),
    ])
}

impl Core {
    fn model(&self) -> MutexGuard<'_, Model> {
        lock(&self.shared.model)
    }

    fn with_grid<T>(&self, tab: Tab, f: impl FnOnce(&mut Grid) -> T) -> T {
        f(self.model().grids.entry(tab).or_default())
    }

    /// Loads the settings into the model unless they already are.
    fn loaded(&self) -> MutexGuard<'_, Model> {
        if self.model().settings.is_none() {
            let _ = self.settings();
        }
        self.model()
    }

    /// The first exported asset under `export_dir`, read once per folder.
    fn sample(&self, export_dir: &str) -> Option<(Asset, String)> {
        if let Some((dir, sample)) = &self.model().sample
            && dir == export_dir
        {
            return sample.clone();
        }
        let sample = App::open()
            .ok()
            .and_then(|app| app.first_exported(Path::new(export_dir)).ok().flatten());
        self.model().sample = Some((export_dir.to_owned(), sample.clone()));
        sample
    }
}

#[uniffi::export]
impl Core {
    #[uniffi::constructor]
    pub fn new(listener: Arc<dyn Listener>) -> Arc<Core> {
        let shared = Arc::new(Shared {
            model: Mutex::new(Model {
                grids: HashMap::new(),
                stats: HashMap::new(),
                args: catalog::tabs()
                    .into_iter()
                    .map(|t| (t, t.default_args()))
                    .collect(),
                dates: Dates::default(),
                status: Status::Idle,
                running: None,
                settings: None,
                dropbox_key: false,
                sample: None,
            }),
            dirty: Mutex::new(HashSet::new()),
            listener,
        });
        Shared::spawn_ticker(&shared);
        Arc::new(Core { shared })
    }

    /// Every tab available on this platform, in sidebar order.
    pub fn tabs(&self) -> Vec<Tab> {
        catalog::tabs()
    }

    /// Template fields, grouped in order.
    pub fn fields(&self) -> Vec<Field> {
        settings::fields()
    }

    pub fn settings(&self) -> Result<Settings, CoreError> {
        let cfg = config::Config::load(&config_file()?)?;
        let s = Settings::from_config(&cfg);
        let mut m = self.model();
        m.settings = Some(s.clone());
        m.dropbox_key = !cfg.dropbox_app_key.trim().is_empty();
        Ok(s)
    }

    /// Saves `settings`; on error nothing is written and the reason is returned.
    pub fn save_settings(&self, settings: Settings) -> Result<(), CoreError> {
        settings.save(&config_file()?)?;
        self.model().settings = Some(settings);
        Ok(())
    }

    /// Renders both templates of `settings` against the first asset exported
    /// to its folder, or a sample.
    pub fn preview(&self, settings: Settings) -> Preview {
        let sample = self.sample(&settings.export_dir);
        settings::preview(&settings, sample.as_ref())
    }

    pub fn run_args(&self, tab: Tab) -> RunArgs {
        self.model()
            .args
            .get(&tab)
            .cloned()
            .unwrap_or_else(|| tab.default_args())
    }

    pub fn set_run_args(&self, tab: Tab, args: RunArgs) {
        self.model().args.insert(tab, args);
        self.shared.mark(Change::Status);
    }

    /// Why `tab` can't run yet, or None when it can.
    pub fn blocker(&self, tab: Tab) -> Option<Blocker> {
        let args = self.run_args(tab);
        let m = self.loaded();
        if let Some(running) = m.running {
            return Some(Blocker::Running { tab: running });
        }
        let export_dir = m.settings.as_ref().map_or("", |s| s.export_dir.as_str());
        if let Some(why) = tab.blocker(&args, export_dir) {
            return Some(why);
        }
        if tab == Tab::Dropbox && !m.dropbox_key {
            return Some(Blocker::DropboxKeyMissing {
                config_path: photosink::config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            });
        }
        None
    }

    /// Ask this before `start` when it returns something.
    pub fn confirmation(&self, tab: Tab) -> Option<Confirmation> {
        tab.confirmation(&self.run_args(tab))
    }

    /// Starts a run on `tab` with its stored args, replacing the tab's rows.
    pub fn start(&self, tab: Tab) -> Result<(), CoreError> {
        if let Some(blocker) = self.blocker(tab) {
            return Err(CoreError::Blocked { blocker });
        }
        let args = self.run_args(tab);
        let template = self
            .model()
            .settings
            .as_ref()
            .map(|s| s.path.clone())
            .unwrap_or_default();
        {
            let mut m = self.model();
            m.stats.remove(&tab);
            let grid = m.grids.entry(tab).or_default();
            grid.clear();
            grid.start_run(tab.run_title(&args, &template));
            if tab == Tab::Dates {
                m.dates.start_scan(!args.arg.trim().is_empty());
                self.shared.mark(Change::Dates);
            }
            self.shared.set_status(&mut m, Status::Running { tab });
            self.shared.mark(Change::Grid { tab });
        }
        let shared = self.shared.clone();
        std::thread::spawn(move || shared.run(tab, args, Job::Run));
        Ok(())
    }

    /// Applies the chosen dates, then scans again unless something failed.
    pub fn apply_dates(&self) -> Result<(), CoreError> {
        if let Some(tab) = self.model().running {
            return Err(CoreError::Running { tab });
        }
        let args = self.run_args(Tab::Dates);
        let choices = {
            let mut m = self.model();
            let choices = m.dates.take_choices();
            if choices.is_empty() {
                return Err(CoreError::NothingChosen);
            }
            m.dates.start_apply();
            m.stats.remove(&Tab::Dates);
            let grid = m.grids.entry(Tab::Dates).or_default();
            grid.clear();
            grid.start_run(RunTitle::Redate {
                count: choices.len() as u32,
            });
            self.shared.set_status(&mut m, Status::ApplyingDates);
            self.shared.mark(Change::Grid { tab: Tab::Dates });
            self.shared.mark(Change::Dates);
            choices
        };
        let shared = self.shared.clone();
        std::thread::spawn(move || shared.run(Tab::Dates, args, Job::ApplyDates(choices)));
        Ok(())
    }

    pub fn status(&self) -> Status {
        self.model().status.clone()
    }

    /// The tab a run is going on, if any; tabs can't be switched meanwhile.
    pub fn running(&self) -> Option<Tab> {
        self.model().running
    }

    pub fn stats(&self, tab: Tab) -> Vec<Stat> {
        self.model().stats.get(&tab).cloned().unwrap_or_default()
    }

    pub fn grid_len(&self, tab: Tab) -> u32 {
        self.with_grid(tab, |g| g.len() as u32)
    }

    pub fn grid_rows(&self, tab: Tab, start: u32, count: u32) -> Vec<Row> {
        self.with_grid(tab, |g| g.rows(start as usize, count as usize))
    }

    pub fn chips(&self, tab: Tab) -> Vec<Chip> {
        self.with_grid(tab, |g| g.chips())
    }

    pub fn hidden_rows(&self, tab: Tab) -> u32 {
        self.with_grid(tab, |g| g.hidden_rows() as u32)
    }

    /// Hides or shows `state`; with `only`, shows just `state`.
    pub fn toggle_chip(&self, tab: Tab, state: RowState, only: bool) {
        self.with_grid(tab, |g| g.toggle(state, only));
        self.shared.mark(Change::Grid { tab });
    }

    pub fn show_all(&self, tab: Tab) {
        self.with_grid(tab, Grid::show_all);
        self.shared.mark(Change::Grid { tab });
    }

    pub fn clear(&self, tab: Tab) {
        let mut m = self.model();
        if m.running == Some(tab) {
            return;
        }
        m.grids.entry(tab).or_default().clear();
        m.stats.remove(&tab);
        self.shared.mark(Change::Grid { tab });
    }

    pub fn dates_view(&self) -> DatesView {
        let m = self.model();
        m.dates.view(m.running.is_some() && !m.dates.scanning())
    }

    pub fn dates_len(&self) -> u32 {
        self.model().dates.len() as u32
    }

    pub fn dates_rows(&self, start: u32, count: u32) -> Vec<DateRow> {
        self.model().dates.rows(start as usize, count as usize)
    }

    pub fn dates_pick(&self, uuid: String, key: DateKey) {
        self.model().dates.pick(&uuid, key);
        self.shared.mark(Change::Dates);
    }

    /// Chooses the user's own date for `uuid`, or drops it with None. `at`
    /// follows the timestamp convention in `dates`.
    pub fn dates_manual(&self, uuid: String, at: Option<i64>) {
        self.model().dates.manual(&uuid, at);
        self.shared.mark(Change::Dates);
    }

    pub fn dates_pick_all(&self, key: DateKey) {
        self.model().dates.pick_all(key);
        self.shared.mark(Change::Dates);
    }

    pub fn dates_clear(&self) {
        self.model().dates.clear_choices();
        self.shared.mark(Change::Dates);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorder(Mutex<Vec<Change>>);

    impl Listener for Recorder {
        fn changed(&self, change: Change) {
            lock(&self.0).push(change);
        }
    }

    #[test]
    fn routes_and_coalesces_changes() {
        let rec = Arc::new(Recorder(Mutex::new(Vec::new())));
        let core = Core::new(rec.clone());
        for n in 1..=50 {
            core.shared.event(
                Tab::Consolidate,
                Event::Asset {
                    n,
                    total: 50,
                    name: format!("a{n}"),
                },
            );
        }
        core.shared.event(
            Tab::Dates,
            Event::Checking {
                total: 9,
                query: None,
            },
        );
        assert_eq!(core.grid_len(Tab::Consolidate), 50);
        assert_eq!(core.grid_len(Tab::Clean), 0, "other tabs untouched");
        std::thread::sleep(TICK * 3);
        let got = lock(&rec.0).clone();
        assert_eq!(
            got.iter()
                .filter(|c| **c
                    == Change::Grid {
                        tab: Tab::Consolidate
                    })
                .count(),
            1
        );
        assert!(got.contains(&Change::Dates));
    }

    #[test]
    fn blocks_runs_without_an_export_folder() {
        let core = Core::new(Arc::new(Recorder(Mutex::new(Vec::new()))));
        let s = Settings::from_config(&config::Config::parse("export_dir = \"\"").unwrap());
        core.model().settings = Some(s);
        assert_eq!(
            core.blocker(Tab::Consolidate),
            Some(Blocker::NoExportFolder)
        );
        assert!(matches!(
            core.start(Tab::Consolidate),
            Err(CoreError::Blocked {
                blocker: Blocker::NoExportFolder
            })
        ));
    }
}
