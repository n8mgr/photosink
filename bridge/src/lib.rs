//! The window's model, shared by every platform's UI. A UI renders what
//! `Core` returns, forwards edits back, and re-reads whatever `Listener`
//! reports changed.

mod automatic;
pub mod catalog;
pub mod dates;
pub mod grid;
pub mod settings;

use automatic::Schedule;
use catalog::{Blocker, Confirmation, RunArgs, RunTitle, Tab};
use dates::{DateKey, DateRow, Dates, DatesView};
use grid::{Chip, Grid, Row, RowState};
use photosink::photo::Asset;
use photosink::redate::Choice;
use photosink::sources::AuthMode;
use photosink::{App, Event, config};
use settings::{Field, Preview, Settings, SettingsProblem};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

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
    args_loaded: bool,
    schedule: Schedule,
    dropbox_key: bool,
    /// The asset templates are previewed against, for the export folder it was read from.
    sample: Option<(String, Option<(Asset, String)>)>,
}

enum Job {
    Run,
    Automatic,
    ApplyDates(Vec<Choice>),
}

struct Shared {
    model: Mutex<Model>,
    dirty: Mutex<HashSet<Change>>,
    listener: Arc<dyn Listener>,
    config_path: Option<PathBuf>,
    config_lock: Mutex<()>,
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

    fn configure_schedule(m: &mut Model) {
        if let Some(settings) = &m.settings {
            m.schedule.configure(settings, &m.args, Instant::now());
        }
    }

    fn blocker(&self, m: &Model, tab: Tab, args: &RunArgs) -> Option<Blocker> {
        if let Some(tab) = m.running {
            return Some(Blocker::Running { tab });
        }
        let export_dir = m.settings.as_ref().map_or("", |s| s.export_dir.as_str());
        if let Some(blocker) = tab.blocker(args, export_dir) {
            return Some(blocker);
        }
        if tab == Tab::Dropbox && !m.dropbox_key {
            return Some(Blocker::DropboxKeyMissing {
                config_path: self
                    .config_path
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(photosink::config_path)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            });
        }
        None
    }

    fn begin_run(&self, m: &mut Model, tab: Tab, args: &RunArgs) {
        let template = m.settings.as_ref().map_or("", |s| s.path.as_str());
        m.stats.remove(&tab);
        let grid = m.grids.entry(tab).or_default();
        grid.clear();
        grid.start_run(tab.run_title(args, template));
        if tab == Tab::Dates {
            m.dates.start_scan(!args.arg.trim().is_empty());
            self.mark(Change::Dates);
        }
        self.set_status(m, Status::Running { tab });
        self.mark(Change::Grid { tab });
    }

    fn scheduled_run(&self, now: Instant) -> Option<(Tab, RunArgs)> {
        let mut m = lock(&self.model);
        if m.running.is_some() {
            return None;
        }
        let tab = m.schedule.ready(now)?;
        let mut args = m.args.get(&tab)?.clone();
        args.dry = false;
        args.option = false;
        if self.blocker(&m, tab, &args).is_some() {
            m.schedule.finished(tab, now);
            return None;
        }
        self.begin_run(&mut m, tab, &args);
        Some((tab, args))
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
        let auth = if matches!(job, Job::Automatic) {
            AuthMode::Unattended
        } else {
            AuthMode::Interactive
        };
        let app = match App::open() {
            Ok(app) => app,
            Err(e) => {
                self.finish(tab, Err(e), applying);
                return;
            }
        };
        let log = match app.run_log(if applying { "dates-apply" } else { tab.slug() }) {
            Ok(log) => log,
            Err(e) => {
                self.finish(tab, Err(e), applying);
                return;
            }
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
            Job::Run | Job::Automatic => match tab {
                #[cfg(target_os = "macos")]
                Tab::ApplePhotos => app
                    .apple_photos_with_auth(Path::new(&args.arg), &progress, dry, auth)
                    .map(|s| summary(&s, dry)),
                #[cfg(not(target_os = "macos"))]
                Tab::ApplePhotos => unreachable!("start() returns Blocker::Unavailable"),
                Tab::OneDrive => app
                    .onedrive_with_auth(args.arg.trim(), &progress, dry, auth)
                    .map(|s| summary(&s, dry)),
                Tab::Dropbox => app
                    .dropbox_with_auth(args.arg.trim(), &progress, dry, auth)
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
        if self.finish(tab, result, applying) {
            self.rescan(args);
        }
    }

    /// Ends a run on `tab`; `applied` when it applied dates rather than scanning.
    fn finish(&self, tab: Tab, result: anyhow::Result<Vec<Stat>>, applied: bool) -> bool {
        let mut m = lock(&self.model);
        let rescan = applied && result.is_ok() && m.grids.get(&tab).map_or(0, Grid::errors) == 0;
        m.schedule.finished(tab, Instant::now());
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
                self.set_status(
                    &mut m,
                    if rescan {
                        Status::CheckingDates
                    } else {
                        Status::Done { errors }
                    },
                );
            }
            Err(e) => {
                m.grids.entry(tab).or_default().push_error(format!("{e:#}"));
                self.set_status(&mut m, Status::Failed);
            }
        }
        self.mark(Change::Grid { tab });
        rescan
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
                if let Some((tab, args)) = shared.scheduled_run(Instant::now()) {
                    let worker = shared.clone();
                    std::thread::spawn(move || worker.run(tab, args, Job::Automatic));
                }
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
    fn create(
        listener: Arc<dyn Listener>,
        config_path: Option<PathBuf>,
        ticker: bool,
    ) -> Arc<Core> {
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
                args_loaded: false,
                schedule: Schedule::default(),
                dropbox_key: false,
                sample: None,
            }),
            dirty: Mutex::new(HashSet::new()),
            listener,
            config_path,
            config_lock: Mutex::new(()),
        });
        if ticker {
            Shared::spawn_ticker(&shared);
        }
        Arc::new(Core { shared })
    }

    fn config_file(&self) -> anyhow::Result<PathBuf> {
        if let Some(path) = &self.shared.config_path {
            if !path.exists() {
                config::write_initial(path)?;
            }
            Ok(path.clone())
        } else {
            config_file()
        }
    }

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
        Self::create(listener, None, true)
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
        let _config = lock(&self.shared.config_lock);
        let cfg = config::Config::load(&self.config_file()?)?;
        let s = Settings::from_config(&cfg);
        let mut m = self.model();
        if !m.args_loaded {
            for (tab, args) in &mut m.args {
                if let Some(source) = cfg.sources.get(tab.slug()) {
                    args.arg = source.arg.clone();
                    args.automatic = source.automatic;
                }
            }
            m.args_loaded = true;
        }
        m.settings = Some(s.clone());
        m.dropbox_key = !cfg.dropbox_app_key.trim().is_empty();
        Shared::configure_schedule(&mut m);
        Ok(s)
    }

    /// Saves `settings`; on error nothing is written and the reason is returned.
    pub fn save_settings(&self, settings: Settings) -> Result<(), CoreError> {
        drop(self.loaded());
        let _config = lock(&self.shared.config_lock);
        let mut m = self.model();
        settings.save(&self.config_file()?)?;
        m.settings = Some(settings);
        Shared::configure_schedule(&mut m);
        self.shared.mark(Change::Status);
        Ok(())
    }

    pub fn automatic_enabled(&self) -> bool {
        self.model().schedule.enabled()
    }

    /// Renders both templates of `settings` against the first asset exported
    /// to its folder, or a sample.
    pub fn preview(&self, settings: Settings) -> Preview {
        let sample = self.sample(&settings.export_dir);
        settings::preview(&settings, sample.as_ref())
    }

    pub fn run_args(&self, tab: Tab) -> RunArgs {
        self.loaded()
            .args
            .get(&tab)
            .cloned()
            .unwrap_or_else(|| tab.default_args())
    }

    pub fn set_run_args(&self, tab: Tab, mut args: RunArgs) -> Result<(), CoreError> {
        drop(self.loaded());
        let _config = lock(&self.shared.config_lock);
        let mut m = self.model();
        if tab.is_source() {
            let source = toml::Value::try_from(config::SourceSettings {
                arg: args.arg.clone(),
                automatic: args.automatic,
            })
            .map_err(anyhow::Error::from)?;
            config::update(&self.config_file()?, |t| {
                let sources = t
                    .entry("sources")
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                if let Some(sources) = sources.as_table_mut() {
                    sources.insert(tab.slug().into(), source);
                }
            })?;
        } else {
            args.automatic = false;
        }
        let changed_path = m.args.get(&tab).is_some_and(|old| old.arg != args.arg);
        m.args.insert(tab, args);
        Shared::configure_schedule(&mut m);
        if changed_path {
            m.schedule.finished(tab, Instant::now());
        }
        self.shared.mark(Change::Status);
        Ok(())
    }

    /// Why `tab` can't run yet, or None when it can.
    pub fn blocker(&self, tab: Tab) -> Option<Blocker> {
        let args = self.run_args(tab);
        let m = self.loaded();
        self.shared.blocker(&m, tab, &args)
    }

    /// Ask this before `start` when it returns something.
    pub fn confirmation(&self, tab: Tab) -> Option<Confirmation> {
        tab.confirmation(&self.run_args(tab))
    }

    /// Starts a run on `tab` with its stored args, replacing the tab's rows.
    pub fn start(&self, tab: Tab) -> Result<(), CoreError> {
        drop(self.loaded());
        let args = {
            let mut m = self.model();
            let args = m
                .args
                .get(&tab)
                .cloned()
                .unwrap_or_else(|| tab.default_args());
            if let Some(blocker) = self.shared.blocker(&m, tab, &args) {
                return Err(CoreError::Blocked { blocker });
            }
            self.shared.begin_run(&mut m, tab, &args);
            args
        };
        let shared = self.shared.clone();
        std::thread::spawn(move || shared.run(tab, args, Job::Run));
        Ok(())
    }

    /// Applies the chosen dates, then scans again unless something failed.
    pub fn apply_dates(&self) -> Result<(), CoreError> {
        let args = self.run_args(Tab::Dates);
        let choices = {
            let mut m = self.model();
            if let Some(tab) = m.running {
                return Err(CoreError::Running { tab });
            }
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

    /// The tab with an active operation, if any.
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

    pub fn photos_id(&self, tab: Tab, index: u32) -> Option<String> {
        if tab != Tab::ApplePhotos {
            return None;
        }
        self.with_grid(tab, |g| g.source_id(index as usize))
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

    fn isolated_core(path: &Path) -> Arc<Core> {
        Core::create(
            Arc::new(Recorder(Mutex::new(Vec::new()))),
            Some(path.to_owned()),
            false,
        )
    }

    #[test]
    fn persists_source_selection_but_not_dry_run_or_maintenance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "export_dir = '/out'\n").unwrap();
        let core = isolated_core(&path);
        core.set_run_args(
            Tab::OneDrive,
            RunArgs {
                arg: "Pictures/Trips".into(),
                automatic: true,
                dry: true,
                option: true,
            },
        )
        .unwrap();
        core.set_run_args(
            Tab::Clean,
            RunArgs {
                automatic: true,
                ..RunArgs::default()
            },
        )
        .unwrap();
        assert!(!core.run_args(Tab::Clean).automatic);
        let mut settings = core.settings().unwrap();
        settings.automatic = true;
        core.save_settings(settings).unwrap();
        let reopened = isolated_core(&path);
        let args = reopened.run_args(Tab::OneDrive);
        assert_eq!(args.arg, "Pictures/Trips");
        assert!(args.automatic);
        assert!(!args.dry && !args.option);
        assert!(reopened.automatic_enabled());
        assert_eq!(config::Config::load(&path).unwrap().sources.len(), 1);
    }

    #[test]
    fn automatic_runs_reserve_the_same_slot_as_manual_operations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "export_dir = '/out'\nautomatic = true\n[sources.onedrive]\nautomatic = true\n",
        )
        .unwrap();
        let core = isolated_core(&path);
        let mut args = core.run_args(Tab::OneDrive);
        args.dry = true;
        core.set_run_args(Tab::OneDrive, args).unwrap();
        let now = Instant::now();
        let (tab, args) = core.shared.scheduled_run(now).unwrap();
        assert_eq!(tab, Tab::OneDrive);
        assert!(!args.dry);
        assert!(core.shared.scheduled_run(now).is_none());
        assert!(matches!(
            core.start(Tab::Consolidate),
            Err(CoreError::Blocked {
                blocker: Blocker::Running { tab: Tab::OneDrive }
            })
        ));
        assert!(matches!(
            core.apply_dates(),
            Err(CoreError::Running { tab: Tab::OneDrive })
        ));
        core.shared
            .finish(tab, Err(anyhow::anyhow!("offline")), false);
        assert!(core.shared.scheduled_run(Instant::now()).is_none());
        assert!(
            core.shared
                .scheduled_run(Instant::now() + Duration::from_secs(901))
                .is_some()
        );
    }

    #[test]
    fn manual_run_and_date_rescan_keep_automatic_work_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "export_dir = '/out'\nautomatic = true\n[sources.onedrive]\nautomatic = true\n",
        )
        .unwrap();
        let core = isolated_core(&path);
        core.settings().unwrap();
        {
            let mut m = core.model();
            core.shared.set_status(&mut m, Status::ApplyingDates);
        }
        assert!(core.shared.scheduled_run(Instant::now()).is_none());
        assert!(core.shared.finish(Tab::Dates, Ok(Vec::new()), true));
        assert_eq!(core.status(), Status::CheckingDates);
        assert!(core.shared.scheduled_run(Instant::now()).is_none());
        core.shared.finish(Tab::Dates, Ok(Vec::new()), false);
        assert_eq!(
            core.shared.scheduled_run(Instant::now()).unwrap().0,
            Tab::OneDrive
        );
    }

    #[test]
    fn disabling_automatic_checks_preserves_sources_and_cancels_pending_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "export_dir = '/out'\nautomatic = true\n[sources.onedrive]\nautomatic = true\n",
        )
        .unwrap();
        let core = isolated_core(&path);
        let mut settings = core.settings().unwrap();
        assert!(core.automatic_enabled());
        settings.automatic = false;
        core.save_settings(settings).unwrap();
        assert!(!core.automatic_enabled());
        assert!(core.run_args(Tab::OneDrive).automatic);
        assert!(
            core.shared
                .scheduled_run(Instant::now() + Duration::from_secs(3600))
                .is_none()
        );
    }

    #[test]
    fn editing_a_watched_path_defers_the_next_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "export_dir = '/out'\nautomatic = true\n[sources.onedrive]\nautomatic = true\n",
        )
        .unwrap();
        let core = isolated_core(&path);
        let mut args = core.run_args(Tab::OneDrive);
        args.arg = "Pictures/".into();
        core.set_run_args(Tab::OneDrive, args.clone()).unwrap();
        assert!(core.shared.scheduled_run(Instant::now()).is_none());
        args.arg.push_str("Trips");
        core.set_run_args(Tab::OneDrive, args).unwrap();
        assert!(
            core.shared
                .scheduled_run(Instant::now() + Duration::from_secs(899))
                .is_none()
        );
        let (_, args) = core
            .shared
            .scheduled_run(Instant::now() + Duration::from_secs(901))
            .unwrap();
        assert_eq!(args.arg, "Pictures/Trips");
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
                    source_id: None,
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
