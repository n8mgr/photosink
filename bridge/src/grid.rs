use crate::catalog::RunTitle;
use crate::dates::stamp;
use photosink::Event;
use photosink::clean::Reason;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum RowState {
    Exported,
    Moved,
    Redated,
    Duplicate,
    Metadata,
    Unchanged,
    Missing,
    Abandoned,
    Unclaimed,
    Unreferenced,
    Failed,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CleanReason {
    /// A state row whose file is not on disk.
    Missing,
    /// The sidecar of an asset left with no files.
    Abandoned,
    /// A sidecar no live asset claims.
    Unclaimed,
    /// A media file no state row references.
    Unreferenced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CleanOutcome {
    Removed,
    /// Left in place: an unreferenced file without the option to delete it.
    Kept,
    /// A dry run's would-be removal.
    WouldRemove,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Detail {
    /// Where a file landed, exported or moved.
    Placed { path: String },
    /// The existing file a duplicate matched.
    Duplicate { existing: String },
    /// A file that had metadata written into it.
    Metadata { path: String },
    /// A new taken date; see `dates` for the timestamp convention.
    Redated { from: Option<i64>, to: i64 },
    /// `subject` is set when it differs from the row's name.
    Error {
        subject: Option<String>,
        message: String,
    },
    /// An asset the run had nothing to do for.
    NothingNew,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Fact {
    /// `total` assets in `place`; `name` is its last path component.
    Found {
        total: u64,
        place: String,
        name: String,
    },
    VisibleToPhotoKit {
        total: u64,
    },
    /// New or changed since the last run.
    Changed {
        total: u64,
    },
    Inventory {
        files: u64,
        sidecars: u64,
        dir: String,
    },
    Warning {
        text: String,
    },
    /// The current phase: listing `path`; `name` is its last path component.
    Listing {
        path: String,
        name: String,
    },
    /// The current phase: reading `path`; `name` is its last path component.
    Reading {
        path: String,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SignIn {
    /// Approve access in the browser; the run continues on its own.
    Browser,
    /// Enter `code` at the URL.
    DeviceCode { code: String },
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Position {
    pub n: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Row {
    /// The header of a run. The current phase, if any, is the last fact.
    /// `title` is None for events that arrived before any run started.
    Run {
        title: Option<RunTitle>,
        facts: Vec<Fact>,
        log_path: String,
    },
    /// `name` is None until the run names the asset; `position` is None on
    /// redated rows.
    Asset {
        position: Option<Position>,
        name: Option<String>,
        state: RowState,
        details: Vec<Detail>,
        failed: bool,
    },
    Clean {
        path: String,
        reason: CleanReason,
        outcome: CleanOutcome,
        failed: bool,
    },
    /// An error not tied to one asset.
    Error {
        subject: Option<String>,
        message: String,
    },
    /// A sign-in waiting on the user at `url`.
    SignIn { prompt: SignIn, url: String },
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Chip {
    pub state: RowState,
    pub count: u32,
    pub hidden: bool,
}

/// Every state, in chip order.
const STATES: &[RowState] = &[
    RowState::Exported,
    RowState::Moved,
    RowState::Redated,
    RowState::Duplicate,
    RowState::Metadata,
    RowState::Unchanged,
    RowState::Missing,
    RowState::Abandoned,
    RowState::Unclaimed,
    RowState::Unreferenced,
    RowState::Failed,
    RowState::Other,
];

#[derive(Debug, Clone)]
enum Item {
    Run {
        title: Option<RunTitle>,
        facts: Vec<Fact>,
        phase: Option<Fact>,
        log: String,
    },
    Asset {
        position: Option<(usize, usize)>,
        name: String,
        details: Vec<Detail>,
        moved: bool,
        redated: bool,
        failed: bool,
    },
    Clean {
        path: String,
        reason: Reason,
        removed: bool,
    },
    Error {
        subject: String,
        message: String,
    },
    SignIn {
        code: String,
        url: String,
    },
}

/// One tab's rows, grouped from its events: every event for asset `n` of a
/// run lands on that asset's row.
#[derive(Debug, Default)]
pub(crate) struct Grid {
    items: Vec<Item>,
    by_tag: HashMap<usize, usize>,
    run: Option<usize>,
    hidden: HashSet<RowState>,
    visible: Option<Vec<usize>>,
}

fn base(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_owned())
}

fn reason(r: Reason) -> CleanReason {
    match r {
        Reason::Missing => CleanReason::Missing,
        Reason::Abandoned => CleanReason::Abandoned,
        Reason::Unclaimed => CleanReason::Unclaimed,
        Reason::Unreferenced => CleanReason::Unreferenced,
    }
}

fn nonempty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_owned())
}

impl Item {
    fn state(&self) -> Option<RowState> {
        Some(match self {
            Item::Run { .. } => return None,
            Item::Clean { reason, .. } => match reason {
                Reason::Missing => RowState::Missing,
                Reason::Abandoned => RowState::Abandoned,
                Reason::Unclaimed => RowState::Unclaimed,
                Reason::Unreferenced => RowState::Unreferenced,
            },
            Item::Error { .. } => RowState::Failed,
            Item::SignIn { .. } => RowState::Other,
            Item::Asset {
                details,
                moved,
                redated,
                failed,
                ..
            } => {
                let has = |f: fn(&Detail) -> bool| details.iter().any(f);
                if *failed {
                    RowState::Failed
                } else if *redated {
                    RowState::Redated
                } else if *moved {
                    RowState::Moved
                } else if has(|d| matches!(d, Detail::Placed { .. })) {
                    RowState::Exported
                } else if has(|d| matches!(d, Detail::Duplicate { .. })) {
                    RowState::Duplicate
                } else if has(|d| matches!(d, Detail::Metadata { .. })) {
                    RowState::Metadata
                } else {
                    RowState::Unchanged
                }
            }
        })
    }

    fn row(&self) -> Row {
        match self {
            Item::Run {
                title,
                facts,
                phase,
                log,
            } => Row::Run {
                title: title.clone(),
                facts: facts.iter().chain(phase).cloned().collect(),
                log_path: log.clone(),
            },
            Item::Asset {
                position,
                name,
                details,
                failed,
                ..
            } => {
                let mut details = details.clone();
                if details.is_empty() && !failed {
                    details.push(Detail::NothingNew);
                }
                Row::Asset {
                    position: position.map(|(n, total)| Position {
                        n: n as u64,
                        total: total as u64,
                    }),
                    name: nonempty(name),
                    state: self.state().unwrap_or(RowState::Other),
                    details,
                    failed: *failed,
                }
            }
            Item::Clean {
                path,
                reason: r,
                removed,
            } => Row::Clean {
                path: path.clone(),
                reason: reason(*r),
                outcome: if *removed {
                    CleanOutcome::Removed
                } else if *r == Reason::Unreferenced {
                    CleanOutcome::Kept
                } else {
                    CleanOutcome::WouldRemove
                },
                failed: *r == Reason::Missing,
            },
            Item::Error { subject, message } => Row::Error {
                subject: nonempty(subject),
                message: message.clone(),
            },
            Item::SignIn { code, url } => Row::SignIn {
                prompt: if code.is_empty() {
                    SignIn::Browser
                } else {
                    SignIn::DeviceCode { code: code.clone() }
                },
                url: url.clone(),
            },
        }
    }
}

impl Grid {
    fn changed(&mut self) {
        self.visible = None;
    }

    fn header(&mut self) -> usize {
        match self.run {
            Some(i) => i,
            None => self.push_run(None),
        }
    }

    fn asset(&mut self, n: usize, total: usize) -> &mut Item {
        let i = match self.by_tag.get(&n) {
            Some(&i) => i,
            None => {
                self.items.push(Item::Asset {
                    position: Some((n, total)),
                    name: String::new(),
                    details: Vec::new(),
                    moved: false,
                    redated: false,
                    failed: false,
                });
                let i = self.items.len() - 1;
                self.by_tag.insert(n, i);
                i
            }
        };
        &mut self.items[i]
    }

    fn detail(&mut self, n: usize, total: usize, detail: Detail) {
        if let Item::Asset { details, .. } = self.asset(n, total) {
            details.push(detail);
        }
    }

    fn fact(&mut self, fact: Fact) {
        let i = self.header();
        if let Item::Run { facts, .. } = &mut self.items[i] {
            facts.push(fact);
        }
    }

    fn visible(&mut self) -> &[usize] {
        if self.visible.is_none() {
            let v = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| it.state().is_none_or(|s| !self.hidden.contains(&s)))
                .map(|(i, _)| i)
                .collect();
            self.visible = Some(v);
        }
        self.visible.as_deref().unwrap_or_default()
    }

    fn push_run(&mut self, title: Option<RunTitle>) -> usize {
        self.by_tag.clear();
        self.items.push(Item::Run {
            title,
            facts: Vec::new(),
            phase: None,
            log: String::new(),
        });
        let i = self.items.len() - 1;
        self.run = Some(i);
        self.changed();
        i
    }

    pub fn start_run(&mut self, title: RunTitle) {
        self.push_run(Some(title));
    }

    pub fn push_error(&mut self, message: String) {
        self.items.push(Item::Error {
            subject: String::new(),
            message,
        });
        self.changed();
    }

    pub fn apply(&mut self, e: &Event) {
        match e {
            Event::Log { path } => {
                let i = self.header();
                if let Item::Run { log, .. } = &mut self.items[i] {
                    *log = path.clone();
                }
            }
            Event::Found { total, place } => self.fact(Fact::Found {
                total: *total as u64,
                place: place.clone(),
                name: base(place),
            }),
            Event::Visible { total } => self.fact(Fact::VisibleToPhotoKit {
                total: *total as u64,
            }),
            Event::Warning { text } => self.fact(Fact::Warning { text: text.clone() }),
            Event::Changed { total } => self.fact(Fact::Changed {
                total: *total as u64,
            }),
            Event::Inventory {
                files,
                sidecars,
                dir,
            } => self.fact(Fact::Inventory {
                files: *files as u64,
                sidecars: *sidecars as u64,
                dir: dir.clone(),
            }),
            Event::Listing { path } | Event::Reading { path } => {
                let (path, name) = (path.clone(), base(path));
                let fact = if matches!(e, Event::Listing { .. }) {
                    Fact::Listing { path, name }
                } else {
                    Fact::Reading { path, name }
                };
                let i = self.header();
                if let Item::Run { phase, .. } = &mut self.items[i] {
                    *phase = Some(fact);
                }
            }
            Event::SignIn { url, code } => self.items.push(Item::SignIn {
                code: code.clone(),
                url: url.clone(),
            }),
            Event::Authorize { url } => self.items.push(Item::SignIn {
                code: String::new(),
                url: url.clone(),
            }),
            Event::Clean {
                path,
                reason,
                removed,
            } => self.items.push(Item::Clean {
                path: path.clone(),
                reason: *reason,
                removed: *removed,
            }),
            Event::Asset {
                n,
                total,
                name: new,
            } => {
                if let Item::Asset { name, .. } = self.asset(*n, *total) {
                    *name = new.clone();
                }
            }
            Event::Placed { n, total, dest, .. } => {
                self.detail(*n, *total, Detail::Placed { path: dest.clone() })
            }
            Event::Duplicate {
                n, total, existing, ..
            } => self.detail(
                *n,
                *total,
                Detail::Duplicate {
                    existing: existing.clone(),
                },
            ),
            Event::Embedded { n, total, path } => {
                self.detail(*n, *total, Detail::Metadata { path: path.clone() })
            }
            Event::Moved {
                n,
                total,
                stem,
                dest,
            } => {
                if let Item::Asset {
                    name,
                    details,
                    moved,
                    ..
                } = self.asset(*n, *total)
                {
                    *name = stem.clone();
                    *moved = true;
                    details.push(Detail::Placed { path: dest.clone() });
                }
            }
            Event::Redated { stem, from, to } => self.items.push(Item::Asset {
                position: None,
                name: stem.clone(),
                details: vec![Detail::Redated {
                    from: from.map(stamp),
                    to: stamp(*to),
                }],
                moved: false,
                redated: true,
                failed: false,
            }),
            Event::Error {
                n: Some(n),
                total: Some(total),
                subject,
                text,
            } => {
                if let Item::Asset {
                    name,
                    details,
                    failed,
                    ..
                } = self.asset(*n, *total)
                {
                    if name.is_empty() {
                        *name = subject.clone();
                    }
                    details.push(Detail::Error {
                        subject: (name != subject).then(|| subject.clone()),
                        message: text.clone(),
                    });
                    *failed = true;
                }
            }
            Event::Error { subject, text, .. } => self.items.push(Item::Error {
                subject: subject.clone(),
                message: text.clone(),
            }),
            Event::Checking { .. } | Event::Checked { .. } | Event::Flagged(_) => return,
        }
        self.changed();
    }

    pub fn len(&mut self) -> usize {
        self.visible().len()
    }

    pub fn rows(&mut self, start: usize, count: usize) -> Vec<Row> {
        let idx: Vec<usize> = self
            .visible()
            .iter()
            .skip(start)
            .take(count)
            .copied()
            .collect();
        idx.into_iter().map(|i| self.items[i].row()).collect()
    }

    /// One chip per possible state.
    pub fn chips(&self) -> Vec<Chip> {
        let mut counts: HashMap<RowState, u32> = HashMap::new();
        for it in &self.items {
            if let Some(s) = it.state() {
                *counts.entry(s).or_default() += 1;
            }
        }
        STATES
            .iter()
            .map(|s| Chip {
                state: *s,
                count: counts.get(s).copied().unwrap_or_default(),
                hidden: self.hidden.contains(s),
            })
            .collect()
    }

    pub fn hidden_rows(&mut self) -> usize {
        let total = self.items.len();
        total - self.visible().len()
    }

    /// Hides or shows `state`; with `only`, shows just `state`.
    pub fn toggle(&mut self, state: RowState, only: bool) {
        if only {
            self.hidden = STATES.iter().copied().filter(|s| *s != state).collect();
        } else if !self.hidden.remove(&state) {
            self.hidden.insert(state);
        }
        self.changed();
    }

    pub fn show_all(&mut self) {
        self.hidden.clear();
        self.changed();
    }

    pub fn clear(&mut self) {
        *self = Grid {
            hidden: std::mem::take(&mut self.hidden),
            ..Grid::default()
        };
    }

    pub fn errors(&self) -> usize {
        self.items
            .iter()
            .filter(|it| it.state() == Some(RowState::Failed))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Tab;

    fn title() -> RunTitle {
        RunTitle::Run {
            tab: Tab::Consolidate,
            arg: String::new(),
            dry: false,
            option: false,
            template: String::new(),
        }
    }

    fn placed(n: usize) -> Event {
        Event::Placed {
            n,
            total: 3,
            file: "a".into(),
            dest: format!("2019/05/{n}.HEIC"),
        }
    }

    fn details(row: &Row) -> &[Detail] {
        match row {
            Row::Asset { details, .. } => details,
            r => panic!("not an asset row: {r:?}"),
        }
    }

    #[test]
    fn groups_late_details_onto_their_asset() {
        let mut g = Grid::default();
        g.start_run(title());
        g.apply(&Event::Log {
            path: "/l.log".into(),
        });
        g.apply(&Event::Found {
            total: 3,
            place: "/p/Lib.photoslibrary".into(),
        });
        for n in 1..=3 {
            g.apply(&Event::Asset {
                n,
                total: 3,
                name: format!("IMG_{n}"),
            });
        }
        g.apply(&placed(2));
        g.apply(&Event::Duplicate {
            n: 3,
            total: 3,
            file: "a".into(),
            existing: "x".into(),
        });
        g.apply(&Event::Error {
            n: Some(1),
            total: Some(3),
            subject: "IMG_1".into(),
            text: "boom".into(),
        });
        assert_eq!(g.len(), 4);
        let rows = g.rows(0, 10);
        assert_eq!(
            rows[0],
            Row::Run {
                title: Some(title()),
                facts: vec![Fact::Found {
                    total: 3,
                    place: "/p/Lib.photoslibrary".into(),
                    name: "Lib.photoslibrary".into(),
                }],
                log_path: "/l.log".into(),
            }
        );
        assert!(matches!(rows[1], Row::Asset { failed: true, .. }));
        assert_eq!(
            details(&rows[1]),
            [Detail::Error {
                subject: None,
                message: "boom".into()
            }]
        );
        assert!(matches!(details(&rows[2])[0], Detail::Placed { .. }));
        assert!(matches!(details(&rows[3])[0], Detail::Duplicate { .. }));
        assert_eq!(
            g.chips()
                .iter()
                .filter(|c| c.count > 0)
                .map(|c| c.state)
                .collect::<Vec<_>>(),
            [RowState::Exported, RowState::Duplicate, RowState::Failed]
        );
        assert!(matches!(
            rows[2],
            Row::Asset {
                position: Some(Position { n: 2, total: 3 }),
                state: RowState::Exported,
                ..
            }
        ));
    }

    #[test]
    fn unnamed_and_idle_assets() {
        let mut g = Grid::default();
        g.start_run(title());
        g.apply(&placed(1));
        g.apply(&Event::Asset {
            n: 2,
            total: 3,
            name: "IMG_2".into(),
        });
        g.apply(&Event::error("disk", "full"));
        let rows = g.rows(1, 3);
        assert!(matches!(rows[0], Row::Asset { name: None, .. }));
        assert_eq!(details(&rows[1]), [Detail::NothingNew]);
        assert_eq!(
            rows[2],
            Row::Error {
                subject: Some("disk".into()),
                message: "full".into()
            }
        );
    }

    #[test]
    fn filters_unseen_states() {
        let mut g = Grid::default();
        let chips = g.chips();
        assert_eq!(chips.iter().map(|c| c.state).collect::<Vec<_>>(), STATES);
        assert!(chips.iter().all(|c| c.count == 0 && !c.hidden));

        g.toggle(RowState::Exported, false);
        assert!(
            g.chips()
                .iter()
                .any(|c| c.state == RowState::Exported && c.hidden && c.count == 0)
        );
        g.start_run(title());
        g.apply(&placed(1));
        assert_eq!(
            g.len(),
            1,
            "run header stays; arriving exported row is hidden"
        );
        assert_eq!(g.hidden_rows(), 1);
        g.show_all();
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn filters_by_state() {
        let mut g = Grid::default();
        g.start_run(title());
        for n in 1..=3 {
            g.apply(&Event::Asset {
                n,
                total: 3,
                name: format!("IMG_{n}"),
            });
        }
        assert_eq!(g.chips().len(), STATES.len());
        g.apply(&placed(1));
        let chips = g.chips();
        assert_eq!(
            chips
                .iter()
                .filter(|c| c.count > 0)
                .map(|c| (c.state, c.count))
                .collect::<Vec<_>>(),
            [(RowState::Exported, 1), (RowState::Unchanged, 2)]
        );
        g.toggle(RowState::Unchanged, false);
        assert_eq!(g.len(), 2, "run header stays");
        assert_eq!(g.hidden_rows(), 2);
        g.toggle(RowState::Unchanged, true);
        assert_eq!(g.len(), 3);
        g.show_all();
        assert_eq!(g.len(), 4);
    }

    #[test]
    fn a_new_run_starts_fresh_numbering() {
        let mut g = Grid::default();
        g.start_run(title());
        g.apply(&placed(1));
        g.start_run(title());
        g.apply(&placed(1));
        assert_eq!(g.len(), 4);
    }

    #[test]
    fn clean_rows() {
        let mut g = Grid::default();
        g.start_run(title());
        g.apply(&Event::Clean {
            path: "a.xmp".into(),
            reason: Reason::Unclaimed,
            removed: false,
        });
        g.apply(&Event::Clean {
            path: "b.jpg".into(),
            reason: Reason::Unreferenced,
            removed: false,
        });
        g.apply(&Event::Clean {
            path: "c.jpg".into(),
            reason: Reason::Missing,
            removed: true,
        });
        let rows = g.rows(1, 3);
        assert_eq!(
            rows[0],
            Row::Clean {
                path: "a.xmp".into(),
                reason: CleanReason::Unclaimed,
                outcome: CleanOutcome::WouldRemove,
                failed: false,
            }
        );
        assert!(matches!(
            rows[1],
            Row::Clean {
                outcome: CleanOutcome::Kept,
                ..
            }
        ));
        assert!(matches!(
            rows[2],
            Row::Clean {
                outcome: CleanOutcome::Removed,
                failed: true,
                ..
            }
        ));
    }

    #[test]
    fn sign_in_and_redated_rows() {
        let mut g = Grid::default();
        g.start_run(title());
        g.apply(&Event::SignIn {
            url: "https://u".into(),
            code: "ABC".into(),
        });
        g.apply(&Event::Authorize {
            url: "https://b".into(),
        });
        let at = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap();
        g.apply(&Event::Redated {
            stem: "IMG_1".into(),
            from: None,
            to: at("2008-09-22 18:48:43"),
        });
        let rows = g.rows(1, 3);
        assert_eq!(
            rows[0],
            Row::SignIn {
                prompt: SignIn::DeviceCode { code: "ABC".into() },
                url: "https://u".into()
            }
        );
        assert!(matches!(
            rows[1],
            Row::SignIn {
                prompt: SignIn::Browser,
                ..
            }
        ));
        assert_eq!(
            details(&rows[2]),
            [Detail::Redated {
                from: None,
                to: 1_222_109_323
            }]
        );
    }
}
