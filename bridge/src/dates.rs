//! The Dates picker. Dates here are wall-clock times without a zone, carried
//! as `i64` seconds since the Unix epoch of that wall-clock time read as UTC:
//! format them in UTC to show the wall-clock time.

use chrono::{DateTime, NaiveDateTime};
use photosink::redate::{Candidate, Choice};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DateKey {
    Create,
    Original,
    Modify,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DateOption {
    pub key: DateKey,
    /// None when the file has no such date.
    pub value: Option<i64>,
    pub chosen: bool,
    /// Same day as the current date, so picking it changes little.
    pub same_day: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DateRow {
    pub uuid: String,
    pub folder: String,
    pub stem: String,
    pub video: bool,
    /// None when the asset has no date.
    pub current: Option<i64>,
    /// Create, Original, Modify, in that order.
    pub options: Vec<DateOption>,
    /// The date set with `dates_manual`, if any.
    pub manual: Option<i64>,
    /// The chosen date is the manual one and no option matches it.
    pub manual_chosen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct ScanProgress {
    pub checked: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DatesView {
    /// Show the picker; otherwise the tab shows its status rows.
    pub shown: bool,
    pub scanning: bool,
    pub applying: bool,
    /// Set once a scan reports how far it got.
    pub scan: Option<ScanProgress>,
    /// 0 to 1 while scanning.
    pub progress: f64,
    /// The scan was a Find with a query, so rows are matches rather than
    /// flagged assets.
    pub query: bool,
    /// Rows listed.
    pub rows: u32,
    /// Rows with a chosen date.
    pub chosen: u32,
    pub can_apply: bool,
    pub can_pick: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Dates {
    rows: Vec<Candidate>,
    choices: HashMap<String, NaiveDateTime>,
    manual: HashMap<String, NaiveDateTime>,
    scan: Option<(usize, usize)>,
    scanning: bool,
    applying: bool,
    shown: bool,
    query: bool,
}

const KEYS: [DateKey; 3] = [DateKey::Create, DateKey::Original, DateKey::Modify];

fn value(c: &Candidate, key: DateKey) -> Option<NaiveDateTime> {
    match key {
        DateKey::Create => c.create,
        DateKey::Original => c.original,
        DateKey::Modify => c.modify,
    }
}

/// `t` as seconds since the epoch, read as UTC.
pub(crate) fn stamp(t: NaiveDateTime) -> i64 {
    t.and_utc().timestamp()
}

fn unstamp(s: i64) -> Option<NaiveDateTime> {
    DateTime::from_timestamp(s, 0).map(|t| t.naive_utc())
}

impl Dates {
    fn find(&self, uuid: &str) -> Option<&Candidate> {
        self.rows.iter().find(|c| c.uuid == uuid)
    }

    pub fn start_scan(&mut self, query: bool) {
        *self = Dates {
            scanning: true,
            shown: true,
            query,
            ..Dates::default()
        };
    }

    /// Hides the picker while the chosen dates are applied.
    pub fn start_apply(&mut self) {
        self.shown = false;
        self.applying = true;
    }

    pub fn finish(&mut self) {
        self.scanning = false;
        self.applying = false;
    }

    pub fn scanning(&self) -> bool {
        self.scanning
    }

    pub fn checked(&mut self, n: usize, total: usize) {
        self.scan = Some((n, total));
    }

    pub fn flagged(&mut self, c: Candidate) {
        self.rows.push(c);
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, c: &Candidate) -> DateRow {
        let chosen = self.choices.get(&c.uuid).copied();
        let options: Vec<DateOption> = KEYS
            .iter()
            .map(|key| {
                let v = value(c, *key);
                DateOption {
                    key: *key,
                    value: v.map(stamp),
                    chosen: v.is_some() && chosen == v,
                    same_day: v.zip(c.current).is_some_and(|(a, b)| a.date() == b.date()),
                }
            })
            .collect();
        DateRow {
            uuid: c.uuid.clone(),
            folder: c.dir.clone(),
            stem: c.stem.clone(),
            video: c.video,
            current: c.current.map(stamp),
            manual_chosen: chosen.is_some() && !options.iter().any(|o| o.chosen),
            options,
            manual: self.manual.get(&c.uuid).copied().map(stamp),
        }
    }

    pub fn rows(&self, start: usize, count: usize) -> Vec<DateRow> {
        self.rows
            .iter()
            .skip(start)
            .take(count)
            .map(|c| self.row(c))
            .collect()
    }

    /// Chooses the row's `key` date, or unchooses it if it was chosen.
    pub fn pick(&mut self, uuid: &str, key: DateKey) {
        let Some(v) = self.find(uuid).and_then(|c| value(c, key)) else {
            return;
        };
        if self.choices.get(uuid) == Some(&v) {
            self.choices.remove(uuid);
        } else {
            self.choices.insert(uuid.to_owned(), v);
        }
    }

    /// Chooses a date of the user's own; None drops it, and the choice too
    /// when the choice was the manual date.
    pub fn manual(&mut self, uuid: &str, at: Option<i64>) {
        if self.find(uuid).is_none() {
            return;
        }
        match at.and_then(unstamp) {
            Some(t) => {
                self.manual.insert(uuid.to_owned(), t);
                self.choices.insert(uuid.to_owned(), t);
            }
            None => {
                self.manual.remove(uuid);
                let typed = self.find(uuid).is_some_and(|c| {
                    let chosen = self.choices.get(uuid).copied();
                    chosen.is_some() && !KEYS.iter().any(|k| value(c, *k) == chosen)
                });
                if typed {
                    self.choices.remove(uuid);
                }
            }
        }
    }

    pub fn pick_all(&mut self, key: DateKey) {
        for c in &self.rows {
            if let Some(v) = value(c, key) {
                self.choices.insert(c.uuid.clone(), v);
            }
        }
    }

    pub fn clear_choices(&mut self) {
        self.choices.clear();
    }

    pub fn take_choices(&mut self) -> Vec<Choice> {
        let mut out: Vec<Choice> = self
            .choices
            .drain()
            .map(|(uuid, taken)| Choice { uuid, taken })
            .collect();
        out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
        self.manual.clear();
        out
    }

    pub fn view(&self, busy: bool) -> DatesView {
        let chosen = self.choices.len();
        DatesView {
            shown: self.shown,
            scanning: self.scanning,
            applying: self.applying,
            scan: self.scan.map(|(n, total)| ScanProgress {
                checked: n as u64,
                total: total as u64,
            }),
            progress: match self.scan {
                Some((n, total)) if total > 0 => n as f64 / total as f64,
                _ => 0.0,
            },
            query: self.query,
            rows: self.rows.len() as u32,
            chosen: chosen as u32,
            can_apply: chosen > 0 && !self.scanning && !busy,
            can_pick: !self.applying,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, photosink::redate::FORMAT).unwrap()
    }

    fn cand(uuid: &str) -> Candidate {
        Candidate {
            uuid: uuid.into(),
            stem: "f1".into(),
            dir: "2016/11".into(),
            video: false,
            current: Some(at("2016-11-28 10:00:00")),
            create: Some(at("2000-09-19 00:33:35")),
            original: Some(at("2000-09-19 00:33:35")),
            modify: Some(at("2016-11-28 18:48:43")),
        }
    }

    #[test]
    fn timestamps_are_wall_clock_read_as_utc() {
        assert_eq!(stamp(at("2008-09-22 18:48:43")), 1_222_109_323);
        assert_eq!(unstamp(1_222_109_323), Some(at("2008-09-22 18:48:43")));
    }

    #[test]
    fn picks_toggle_and_manual_choices() {
        let mut d = Dates::default();
        d.start_scan(false);
        d.flagged(cand("a"));
        d.flagged(cand("b"));
        d.pick("a", DateKey::Create);
        let r = d.row(&d.rows[0].clone());
        assert_eq!(r.options[0].value, Some(969_323_615));
        assert!(
            r.options[0].chosen && r.options[1].chosen,
            "same value, both lit"
        );
        assert!(r.options[2].same_day);
        d.pick("a", DateKey::Create);
        assert!(d.choices.is_empty(), "second pick unchooses");
        d.manual("b", Some(1_222_109_323));
        let r = d.row(&d.rows[1].clone());
        assert!(r.manual_chosen);
        assert_eq!(r.manual, Some(1_222_109_323));
        d.manual("b", None);
        assert!(d.choices.is_empty(), "clearing drops the manual choice");
        d.pick("b", DateKey::Modify);
        d.manual("b", None);
        assert_eq!(d.choices.len(), 1, "clearing keeps a picked option");
        d.pick_all(DateKey::Modify);
        let v = d.view(false);
        assert_eq!((v.rows, v.chosen, v.query), (2, 2, false));
        assert!(!v.can_apply, "not while scanning");
        d.finish();
        assert!(d.view(false).can_apply);
        assert!(!d.view(true).can_apply);
        let choices = d.take_choices();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].taken, at("2016-11-28 18:48:43"));
    }

    #[test]
    fn view_tracks_scan_progress() {
        let mut d = Dates::default();
        d.start_scan(true);
        assert_eq!(d.view(false).scan, None);
        d.checked(100, 400);
        let v = d.view(false);
        assert_eq!(
            v.scan,
            Some(ScanProgress {
                checked: 100,
                total: 400
            })
        );
        assert_eq!(v.progress, 0.25);
        assert!(v.query && v.scanning);
        d.start_apply();
        let v = d.view(true);
        assert!(v.applying && !v.shown && !v.can_pick);
    }
}
