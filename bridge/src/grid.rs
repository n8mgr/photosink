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
pub enum RowKind {
    /// The header of a run: title, facts and the run log.
    Run,
    Asset,
    Clean,
    /// An error not tied to one asset.
    Error,
    /// A sign-in waiting on the user: `tag` says what to do, `title` is the
    /// code to enter (empty when the browser finishes on its own), `url` where.
    SignIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DetailKind {
    /// Where something now is.
    New,
    /// The existing file a duplicate matched.
    Duplicate,
    /// A file that had metadata written into it.
    Metadata,
    Note,
    Error,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Detail {
    pub kind: DetailKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Fact {
    /// The number, empty for a fact without one.
    pub value: String,
    pub label: String,
    pub tooltip: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Row {
    pub kind: RowKind,
    pub state: Option<RowState>,
    /// `12` of `12/400`.
    pub position: String,
    /// `/400` of `12/400`.
    pub of: String,
    pub title: String,
    /// A short label before the title, the reason on a clean row.
    pub tag: String,
    pub details: Vec<Detail>,
    pub facts: Vec<Fact>,
    pub log_path: String,
    pub url: String,
    pub failed: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Chip {
    pub state: RowState,
    pub label: String,
    pub count: u32,
    pub hidden: bool,
}

const STATES: &[(RowState, &str)] = &[
    (RowState::Exported, "Exported"),
    (RowState::Moved, "Moved"),
    (RowState::Redated, "Redated"),
    (RowState::Duplicate, "Duplicate"),
    (RowState::Metadata, "Metadata only"),
    (RowState::Unchanged, "Nothing new"),
    (RowState::Missing, "Missing file"),
    (RowState::Abandoned, "Orphan sidecar"),
    (RowState::Unclaimed, "Stray sidecar"),
    (RowState::Unreferenced, "Unreferenced"),
    (RowState::Failed, "Failed"),
    (RowState::Other, "Other"),
];

#[derive(Debug, Clone)]
enum Item {
    Run {
        title: String,
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
    Error(String),
    SignIn {
        code: String,
        url: String,
    },
}

/// One tab's rows, grouped from its events: every event for asset `n` of a
/// run lands on that asset's row.
#[derive(Debug, Default)]
pub struct Grid {
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

fn reason_label(r: Reason) -> &'static str {
    match r {
        Reason::Missing => "missing file",
        Reason::Abandoned => "orphan sidecar",
        Reason::Unclaimed => "stray sidecar",
        Reason::Unreferenced => "unreferenced",
    }
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
            Item::Error(_) => RowState::Failed,
            Item::SignIn { .. } => RowState::Other,
            Item::Asset {
                details,
                moved,
                redated,
                failed,
                ..
            } => {
                let has = |k| details.iter().any(|d| d.kind == k);
                if *failed {
                    RowState::Failed
                } else if *redated {
                    RowState::Redated
                } else if *moved {
                    RowState::Moved
                } else if has(DetailKind::New) {
                    RowState::Exported
                } else if has(DetailKind::Duplicate) {
                    RowState::Duplicate
                } else if has(DetailKind::Metadata) {
                    RowState::Metadata
                } else {
                    RowState::Unchanged
                }
            }
        })
    }

    fn row(&self) -> Row {
        let mut row = Row {
            kind: RowKind::Asset,
            state: self.state(),
            position: String::new(),
            of: String::new(),
            title: String::new(),
            tag: String::new(),
            details: Vec::new(),
            facts: Vec::new(),
            log_path: String::new(),
            url: String::new(),
            failed: false,
        };
        match self {
            Item::Run {
                title,
                facts,
                phase,
                log,
            } => {
                row.kind = RowKind::Run;
                row.title = title.clone();
                row.facts = facts.iter().chain(phase).cloned().collect();
                row.log_path = log.clone();
            }
            Item::Asset {
                position,
                name,
                details,
                failed,
                ..
            } => {
                if let Some((n, total)) = position {
                    row.position = n.to_string();
                    row.of = format!("/{total}");
                }
                row.title = if name.is_empty() {
                    "…".into()
                } else {
                    name.clone()
                };
                row.details = details.clone();
                if details.is_empty() && !failed {
                    row.details.push(Detail {
                        kind: DetailKind::Note,
                        text: "nothing new".into(),
                    });
                }
                row.failed = *failed;
            }
            Item::Clean {
                path,
                reason,
                removed,
            } => {
                row.kind = RowKind::Clean;
                row.tag = reason_label(*reason).into();
                row.title = path.clone();
                row.failed = *reason == Reason::Missing;
                row.details.push(Detail {
                    kind: if *removed {
                        DetailKind::New
                    } else {
                        DetailKind::Note
                    },
                    text: if *removed {
                        "removed"
                    } else if *reason == Reason::Unreferenced {
                        "kept"
                    } else {
                        "would remove"
                    }
                    .into(),
                });
            }
            Item::Error(text) => {
                row.kind = RowKind::Error;
                row.title = text.clone();
                row.failed = true;
            }
            Item::SignIn { code, url } => {
                row.kind = RowKind::SignIn;
                row.tag = if code.is_empty() {
                    "Approve access in your browser".into()
                } else {
                    "Sign in with code".into()
                };
                row.title = code.clone();
                row.url = url.clone();
            }
        }
        row
    }
}

impl Grid {
    fn changed(&mut self) {
        self.visible = None;
    }

    fn header(&mut self) -> usize {
        match self.run {
            Some(i) => i,
            None => self.start_run(String::new()),
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

    fn detail(&mut self, n: usize, total: usize, kind: DetailKind, text: String) {
        if let Item::Asset { details, .. } = self.asset(n, total) {
            details.push(Detail { kind, text });
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

    pub fn start_run(&mut self, title: String) -> usize {
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

    pub fn push_error(&mut self, text: String) {
        self.items.push(Item::Error(text));
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
            Event::Found { total, place } => self.fact(Fact {
                value: total.to_string(),
                label: format!("assets in {}", base(place)),
                tooltip: place.clone(),
            }),
            Event::Visible { total } => self.fact(Fact {
                value: total.to_string(),
                label: "visible to PhotoKit".into(),
                tooltip: String::new(),
            }),
            Event::Changed { total } => self.fact(Fact {
                value: total.to_string(),
                label: "new or changed".into(),
                tooltip: String::new(),
            }),
            Event::Inventory {
                files,
                sidecars,
                dir,
            } => self.fact(Fact {
                value: files.to_string(),
                label: format!("files, {sidecars} sidecars"),
                tooltip: dir.clone(),
            }),
            Event::Listing { path } | Event::Reading { path } => {
                let verb = if matches!(e, Event::Listing { .. }) {
                    "listing"
                } else {
                    "reading"
                };
                let i = self.header();
                if let Item::Run { phase, .. } = &mut self.items[i] {
                    *phase = Some(Fact {
                        value: String::new(),
                        label: format!("{verb} {}", base(path)),
                        tooltip: path.clone(),
                    });
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
                self.detail(*n, *total, DetailKind::New, dest.clone())
            }
            Event::Duplicate {
                n, total, existing, ..
            } => self.detail(*n, *total, DetailKind::Duplicate, existing.clone()),
            Event::Embedded { n, total, path } => {
                self.detail(*n, *total, DetailKind::Metadata, path.clone())
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
                    details.push(Detail {
                        kind: DetailKind::New,
                        text: dest.clone(),
                    });
                }
            }
            Event::Redated { stem, from, to } => self.items.push(Item::Asset {
                position: None,
                name: stem.clone(),
                details: vec![
                    Detail {
                        kind: DetailKind::Note,
                        text: from.clone(),
                    },
                    Detail {
                        kind: DetailKind::New,
                        text: to.clone(),
                    },
                ],
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
                    details.push(Detail {
                        kind: DetailKind::Error,
                        text: if name == subject {
                            text.clone()
                        } else {
                            format!("{subject}: {text}")
                        },
                    });
                    *failed = true;
                }
            }
            Event::Error { subject, text, .. } => {
                self.items.push(Item::Error(if subject.is_empty() {
                    text.clone()
                } else {
                    format!("{subject}: {text}")
                }))
            }
            Event::Checking { .. } | Event::Checked { .. } | Event::Flagged(_) => return,
        }
        self.changed();
    }

    pub fn len(&mut self) -> usize {
        self.visible().len()
    }

    pub fn is_empty(&mut self) -> bool {
        self.visible().is_empty()
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

    /// One chip per state present.
    pub fn chips(&self) -> Vec<Chip> {
        let mut counts: HashMap<RowState, u32> = HashMap::new();
        for it in &self.items {
            if let Some(s) = it.state() {
                *counts.entry(s).or_default() += 1;
            }
        }
        STATES
            .iter()
            .filter_map(|(s, label)| {
                counts.get(s).map(|&count| Chip {
                    state: *s,
                    label: (*label).into(),
                    count,
                    hidden: self.hidden.contains(s),
                })
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
            self.hidden = STATES
                .iter()
                .map(|(s, _)| *s)
                .filter(|s| *s != state)
                .collect();
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

    fn placed(n: usize) -> Event {
        Event::Placed {
            n,
            total: 3,
            file: "a".into(),
            dest: format!("2019/05/{n}.HEIC"),
        }
    }

    #[test]
    fn groups_late_details_onto_their_asset() {
        let mut g = Grid::default();
        g.start_run("apple-photos x".into());
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
        assert_eq!(rows[0].kind, RowKind::Run);
        assert_eq!(rows[0].log_path, "/l.log");
        assert_eq!(rows[0].facts[0].label, "assets in Lib.photoslibrary");
        assert_eq!(rows[1].state, Some(RowState::Failed));
        assert_eq!(rows[1].details[0].text, "boom");
        assert_eq!(rows[2].state, Some(RowState::Exported));
        assert_eq!(rows[3].state, Some(RowState::Duplicate));
        assert_eq!(
            (rows[2].position.as_str(), rows[2].of.as_str()),
            ("2", "/3")
        );
    }

    #[test]
    fn filters_by_state() {
        let mut g = Grid::default();
        g.start_run("consolidate".into());
        for n in 1..=3 {
            g.apply(&Event::Asset {
                n,
                total: 3,
                name: format!("IMG_{n}"),
            });
        }
        assert_eq!(g.chips().len(), 1, "a single state can still be hidden");
        g.apply(&placed(1));
        let chips = g.chips();
        assert_eq!(
            chips.iter().map(|c| (c.state, c.count)).collect::<Vec<_>>(),
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
        g.start_run("one".into());
        g.apply(&placed(1));
        g.start_run("two".into());
        g.apply(&placed(1));
        assert_eq!(g.len(), 4);
    }

    #[test]
    fn clean_rows() {
        let mut g = Grid::default();
        g.start_run("clean".into());
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
        let rows = g.rows(1, 2);
        assert_eq!(rows[0].details[0].text, "would remove");
        assert_eq!(rows[0].tag, "stray sidecar");
        assert_eq!(rows[1].details[0].text, "kept");
    }
}
