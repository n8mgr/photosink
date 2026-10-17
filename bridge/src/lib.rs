//! The window's model, shared by every platform's UI. A UI renders what
//! `Core` returns, forwards edits back, and re-reads whatever `Listener`
//! reports changed.

pub mod catalog;
pub mod dates;
pub mod grid;
pub mod settings;

use catalog::{Confirmation, Group, RunArgs, Tab};
use dates::{DateKey, DateRow, Dates, DatesView};
use grid::{Chip, Grid, Row, RowState};
use photosink::{App, Event, config};
use settings::{FieldGroup, Insertion, Options, Preview, Settings, Summaries, TemplateKind};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

uniffi::setup_scaffolding!();

const TICK: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum CoreError {
    #[error("{0}")]
    Failed(String),
}

impl From<anyhow::Error> for CoreError {
    fn from(e: anyhow::Error) -> Self {
        CoreError::Failed(format!("{e:#}"))
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

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Status {
    pub text: String,
    pub error: bool,
    pub running: Option<Tab>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Stat {
    pub label: String,
    pub value: String,
}

struct Model {
    grids: HashMap<Tab, Grid>,
    stats: HashMap<Tab, Vec<Stat>>,
    args: HashMap<Tab, RunArgs>,
    dates: Dates,
    status: Status,
    settings: Option<Settings>,
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

fn stats<T: Into<Vec<(&'static str, usize)>>>(pairs: T) -> Vec<Stat> {
    pairs
        .into()
        .into_iter()
        .map(|(label, v)| Stat {
            label: label.into(),
            value: v.to_string(),
        })
        .collect()
}

fn config_file() -> anyhow::Result<std::path::PathBuf> {
    let path = photosink::config_path(None)?;
    if !path.exists() {
        config::write_initial(&path)?;
    }
    Ok(path)
}

impl Shared {
    fn mark(&self, change: Change) {
        lock(&self.dirty).insert(change);
    }

    fn set_status(
        &self,
        m: &mut Model,
        text: impl Into<String>,
        error: bool,
        running: Option<Tab>,
    ) {
        m.status = Status {
            text: text.into(),
            error,
            running,
        };
        self.mark(Change::Status);
    }

    /// Routes one event from a run on `tab` into the model.
    fn event(&self, tab: Tab, e: Event) {
        let mut m = lock(&self.model);
        match e {
            Event::Checking { total, .. } => {
                m.dates.checking(total);
                self.mark(Change::Dates);
            }
            Event::Checked { n, total, flagged } => {
                m.dates.checked(n, total, flagged);
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

    fn run(self: &Arc<Self>, tab: Tab, args: RunArgs, choices: Vec<photosink::redate::Choice>) {
        let app = match App::open(None, None) {
            Ok(app) => app,
            Err(e) => return self.finish(tab, Err(e), Vec::new()),
        };
        let log = match app.run_log(if choices.is_empty() {
            tab.slug()
        } else {
            "dates-apply"
        }) {
            Ok(log) => log,
            Err(e) => return self.finish(tab, Err(e), Vec::new()),
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
        let result: anyhow::Result<Vec<Stat>> = match tab {
            #[cfg(target_os = "macos")]
            Tab::ApplePhotos => app
                .apple_photos(Path::new(&args.arg), &progress, dry)
                .map(|s| summary(&s, dry)),
            #[cfg(not(target_os = "macos"))]
            Tab::ApplePhotos => Err(anyhow::anyhow!("Apple Photos is only available on macOS")),
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
            Tab::Clean => app.clean(args.option, dry, &progress).map(|c| {
                let mut out = vec![
                    ("rows checked", c.rows),
                    ("dead rows", c.dead_rows),
                    ("assets reset", c.removed_assets),
                    (
                        if dry {
                            "sidecars to remove"
                        } else {
                            "sidecars removed"
                        },
                        c.removed_sidecars,
                    ),
                    ("unreferenced files", c.orphan_files),
                ];
                if args.option {
                    out.push((
                        if dry {
                            "files to remove"
                        } else {
                            "files removed"
                        },
                        c.removed_files,
                    ));
                }
                stats(out)
            }),
            Tab::Dates if !choices.is_empty() => app
                .redate_apply(&choices, &progress)
                .map(|c| consolidated(&c, false)),
            Tab::Dates => {
                let q = args.arg.trim();
                app.redate_scan((!q.is_empty()).then_some(q), &progress)
                    .map(|_| Vec::new())
            }
        };
        let applied = !choices.is_empty();
        self.finish(tab, result, choices);
        if applied {
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

    fn finish(
        &self,
        tab: Tab,
        result: anyhow::Result<Vec<Stat>>,
        applied: Vec<photosink::redate::Choice>,
    ) {
        let mut m = lock(&self.model);
        if tab == Tab::Dates {
            m.dates.scanning = false;
            m.dates.applying = false;
            self.mark(Change::Dates);
        }
        match result {
            Ok(stats) => {
                if !(tab == Tab::Dates && applied.is_empty()) {
                    m.stats.insert(tab, stats);
                }
                let failed = m.grids.get(&tab).map_or(0, Grid::errors);
                if failed > 0 {
                    self.set_status(&mut m, format!("Done with {failed} errors"), true, None);
                } else {
                    self.set_status(&mut m, "Done", false, None);
                }
            }
            Err(e) => {
                m.grids.entry(tab).or_default().push_error(format!("{e:#}"));
                self.set_status(&mut m, "Failed", true, None);
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
            self.set_status(&mut m, "Checking dates…", false, Some(Tab::Dates));
            self.mark(Change::Grid { tab: Tab::Dates });
            self.mark(Change::Dates);
        }
        self.run(Tab::Dates, args, Vec::new());
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
        (if dry { "planned" } else { "exported" }, s.exported),
        ("duplicates", s.duplicates),
        ("skipped", s.skipped),
        ("filtered", s.filtered),
        ("errors", s.errors),
    ])
}

fn consolidated(c: &photosink::consolidate::Consolidated, dry: bool) -> Vec<Stat> {
    stats([
        (if dry { "would move" } else { "moved" }, c.moved),
        ("renamed", c.restemmed),
        ("unchanged", c.unchanged),
        ("no metadata", c.missing),
    ])
}

impl Core {
    fn model(&self) -> MutexGuard<'_, Model> {
        lock(&self.shared.model)
    }

    fn with_grid<T>(&self, tab: Tab, f: impl FnOnce(&mut Grid) -> T) -> T {
        f(self.model().grids.entry(tab).or_default())
    }

    fn export_dir(&self) -> String {
        let known = self.model().settings.as_ref().map(|s| s.export_dir.clone());
        known
            .or_else(|| self.settings().ok().map(|s| s.export_dir))
            .unwrap_or_default()
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
                args: Tab::all()
                    .into_iter()
                    .map(|t| (t, t.default_args()))
                    .collect(),
                dates: Dates::default(),
                status: Status {
                    text: "Idle".into(),
                    error: false,
                    running: None,
                },
                settings: None,
            }),
            dirty: Mutex::new(HashSet::new()),
            listener,
        });
        Shared::spawn_ticker(&shared);
        Arc::new(Core { shared })
    }

    pub fn groups(&self) -> Vec<Group> {
        catalog::groups()
    }

    pub fn options(&self) -> Options {
        settings::options()
    }

    pub fn fields(&self) -> Vec<FieldGroup> {
        settings::fields()
    }

    pub fn settings(&self) -> Result<Settings, CoreError> {
        let cfg = config::Config::load(&config_file()?)?;
        let s = Settings::from_config(&cfg);
        self.model().settings = Some(s.clone());
        Ok(s)
    }

    /// Saves `settings`; on error nothing is written and the reason is returned.
    pub fn save_settings(&self, settings: Settings) -> Result<(), CoreError> {
        settings.save(&config_file()?)?;
        self.model().settings = Some(settings);
        Ok(())
    }

    pub fn summaries(&self, settings: Settings) -> Summaries {
        settings.summaries()
    }

    pub fn preview(&self, settings: Settings, kind: TemplateKind) -> Preview {
        settings::preview(
            &settings.export_dir,
            &settings.path,
            &settings.filename,
            kind,
        )
    }

    /// Inserts `token` over the selection `start..end`, in UTF-16 code units.
    pub fn insert_field(
        &self,
        text: String,
        start: u32,
        end: u32,
        token: String,
        kind: TemplateKind,
    ) -> Insertion {
        settings::insert_field(&text, start, end, &token, kind)
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

    pub fn run_label(&self, tab: Tab) -> String {
        tab.run_label(&self.run_args(tab))
    }

    /// Why `tab` can't run yet, or None when it can.
    pub fn blocker(&self, tab: Tab) -> Option<String> {
        if let Some(running) = self.model().status.running {
            return Some(format!("{} is running.", running.name()));
        }
        if let Some(why) = tab.blocker(&self.run_args(tab), &self.export_dir()) {
            return Some(why);
        }
        if tab == Tab::Dropbox {
            let key = config_file()
                .and_then(|p| config::Config::load(&p))
                .ok()
                .and_then(|c| c.dropbox_app_key)
                .filter(|k| !k.trim().is_empty());
            if key.is_none() {
                return Some(format!(
                    "Set dropbox_app_key in {} to your Dropbox app's key.",
                    photosink::config_path(None)
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                ));
            }
        }
        None
    }

    /// Ask this before `start` when it returns something.
    pub fn confirmation(&self, tab: Tab) -> Option<Confirmation> {
        tab.confirmation(&self.run_args(tab))
    }

    /// Starts a run on `tab` with its stored args, replacing the tab's rows.
    pub fn start(&self, tab: Tab) -> Result<(), CoreError> {
        if let Some(why) = self.blocker(tab) {
            return Err(CoreError::Failed(why));
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
            self.shared
                .set_status(&mut m, format!("Running {}…", tab.name()), false, Some(tab));
            self.shared.mark(Change::Grid { tab });
        }
        let shared = self.shared.clone();
        std::thread::spawn(move || shared.run(tab, args, Vec::new()));
        Ok(())
    }

    /// Applies the chosen dates, then scans again unless something failed.
    pub fn apply_dates(&self) -> Result<(), CoreError> {
        if let Some(running) = self.model().status.running {
            return Err(CoreError::Failed(format!("{} is running.", running.name())));
        }
        let args = self.run_args(Tab::Dates);
        let choices = {
            let mut m = self.model();
            let choices = m.dates.take_choices();
            if choices.is_empty() {
                return Err(CoreError::Failed("Nothing chosen.".into()));
            }
            m.dates.shown = false;
            m.dates.applying = true;
            m.stats.remove(&Tab::Dates);
            let grid = m.grids.entry(Tab::Dates).or_default();
            grid.clear();
            grid.start_run(format!("redate {} assets", choices.len()));
            self.shared
                .set_status(&mut m, "Applying dates…", false, Some(Tab::Dates));
            self.shared.mark(Change::Grid { tab: Tab::Dates });
            self.shared.mark(Change::Dates);
            choices
        };
        let shared = self.shared.clone();
        std::thread::spawn(move || shared.run(Tab::Dates, args, choices));
        Ok(())
    }

    pub fn status(&self) -> Status {
        self.model().status.clone()
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
        if m.status.running == Some(tab) {
            return;
        }
        m.grids.entry(tab).or_default().clear();
        m.stats.remove(&tab);
        self.shared.mark(Change::Grid { tab });
    }

    pub fn dates_view(&self) -> DatesView {
        let m = self.model();
        m.dates
            .view(m.status.running.is_some() && !m.dates.scanning)
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

    /// Chooses a typed date. Returns why it isn't one, if it isn't.
    pub fn dates_manual(&self, uuid: String, text: String) -> Option<String> {
        let out = self.model().dates.manual(&uuid, &text);
        self.shared.mark(Change::Dates);
        out
    }

    pub fn dates_pick_all(&self, key: DateKey) {
        self.model().dates.pick_all(key);
        self.shared.mark(Change::Dates);
    }

    pub fn dates_clear(&self) {
        self.model().dates.clear_choices();
        self.shared.mark(Change::Dates);
    }

    /// Opens a file or folder with the system's default app.
    pub fn open_path(&self, path: String) -> Result<(), CoreError> {
        open::that_detached(&path).map_err(|e| CoreError::Failed(format!("{path}: {e}")))
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
        let mut s = Settings::from_config(&config::Config::parse("export_dir = \"\"").unwrap());
        s.export_dir = String::new();
        core.model().settings = Some(s);
        assert!(core.blocker(Tab::Consolidate).is_some());
        assert!(core.start(Tab::Consolidate).is_err());
    }
}
