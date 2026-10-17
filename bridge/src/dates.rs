use photosink::redate::{self, Candidate, Choice};
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
    pub label: String,
    /// Empty when the file has no such date.
    pub value: String,
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
    pub current: String,
    pub options: Vec<DateOption>,
    pub manual: String,
    pub manual_chosen: bool,
    pub manual_error: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DatesView {
    /// Show the picker; otherwise the tab shows its status rows.
    pub shown: bool,
    pub scanning: bool,
    pub applying: bool,
    /// 0 to 1 while scanning.
    pub progress: f64,
    /// `checked 120/400, 12 flagged, 3 chosen`.
    pub summary: String,
    pub empty_text: String,
    pub apply_label: String,
    pub can_apply: bool,
    pub can_pick: bool,
    pub manual_placeholder: String,
}

#[derive(Debug, Default)]
pub struct Dates {
    rows: Vec<Candidate>,
    choices: HashMap<String, String>,
    manual: HashMap<String, String>,
    manual_error: HashMap<String, String>,
    scan: Option<(usize, usize, usize)>,
    pub scanning: bool,
    pub applying: bool,
    pub shown: bool,
    query: bool,
}

const KEYS: [(DateKey, &str); 3] = [
    (DateKey::Create, "Create"),
    (DateKey::Original, "Original"),
    (DateKey::Modify, "Modify"),
];

fn value(c: &Candidate, key: DateKey) -> Option<&String> {
    match key {
        DateKey::Create => c.create.as_ref(),
        DateKey::Original => c.original.as_ref(),
        DateKey::Modify => c.modify.as_ref(),
    }
}

fn day(s: &str) -> &str {
    s.get(..10).unwrap_or(s)
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

    pub fn checking(&mut self, total: usize) {
        self.scan = Some((0, total, 0));
    }

    pub fn checked(&mut self, n: usize, total: usize, flagged: usize) {
        self.scan = Some((n, total, flagged));
    }

    pub fn flagged(&mut self, c: Candidate) {
        self.rows.push(c);
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, c: &Candidate) -> DateRow {
        let chosen = self.choices.get(&c.uuid);
        let options: Vec<DateOption> = KEYS
            .iter()
            .map(|(key, label)| {
                let v = value(c, *key);
                DateOption {
                    key: *key,
                    label: (*label).into(),
                    value: v.cloned().unwrap_or_default(),
                    chosen: v.is_some() && chosen == v,
                    same_day: v
                        .zip(c.current.as_ref())
                        .is_some_and(|(a, b)| day(a) == day(b)),
                }
            })
            .collect();
        DateRow {
            uuid: c.uuid.clone(),
            folder: c.dir.clone(),
            stem: c.stem.clone(),
            video: c.video,
            current: c.current.clone().unwrap_or_else(|| "no date".into()),
            manual_chosen: chosen.is_some() && !options.iter().any(|o| o.chosen),
            options,
            manual: self.manual.get(&c.uuid).cloned().unwrap_or_default(),
            manual_error: self.manual_error.get(&c.uuid).cloned().unwrap_or_default(),
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
        let Some(v) = self.find(uuid).and_then(|c| value(c, key)).cloned() else {
            return;
        };
        if self.choices.get(uuid) == Some(&v) {
            self.choices.remove(uuid);
        } else {
            self.choices.insert(uuid.to_owned(), v);
            self.manual_error.remove(uuid);
        }
    }

    /// Chooses typed text; empty text drops a typed choice. Returns why the
    /// text isn't a date, if it isn't.
    pub fn manual(&mut self, uuid: &str, text: &str) -> Option<String> {
        let text = text.trim();
        self.manual.insert(uuid.to_owned(), text.to_owned());
        if text.is_empty() {
            self.manual_error.remove(uuid);
            let typed = self.find(uuid).is_some_and(|c| {
                let chosen = self.choices.get(uuid);
                chosen.is_some() && !KEYS.iter().any(|(k, _)| value(c, *k) == chosen)
            });
            if typed {
                self.choices.remove(uuid);
            }
            return None;
        }
        match redate::check(text) {
            Ok(()) => {
                self.manual_error.remove(uuid);
                self.choices.insert(uuid.to_owned(), text.to_owned());
                None
            }
            Err(e) => {
                let e = format!("{e:#}");
                self.manual_error.insert(uuid.to_owned(), e.clone());
                Some(e)
            }
        }
    }

    pub fn pick_all(&mut self, key: DateKey) {
        for c in &self.rows {
            if let Some(v) = value(c, key) {
                self.choices.insert(c.uuid.clone(), v.clone());
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
        let word = if self.query { "matching" } else { "flagged" };
        let chosen = self.choices.len();
        let mut summary = String::new();
        if self.scanning
            && let Some((n, total, _)) = self.scan
        {
            summary.push_str(&format!("checked {n}/{total}, "));
        }
        summary.push_str(&format!("{} {word}, {chosen} chosen", self.rows.len()));
        DatesView {
            shown: self.shown,
            scanning: self.scanning,
            applying: self.applying,
            progress: match self.scan {
                Some((n, total, _)) if total > 0 => n as f64 / total as f64,
                _ => 0.0,
            },
            summary,
            empty_text: if self.scanning {
                "Checking…"
            } else if self.query {
                "Nothing matches."
            } else {
                "Every asset's folder date matches its file."
            }
            .into(),
            apply_label: if self.applying {
                "Applying…".into()
            } else {
                format!("Apply {chosen}")
            },
            can_apply: chosen > 0 && !self.scanning && !busy,
            can_pick: !self.applying,
            manual_placeholder: "YYYY-MM-DD, with time, or RFC 3339".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(uuid: &str) -> Candidate {
        Candidate {
            uuid: uuid.into(),
            stem: "f1".into(),
            dir: "2016/11".into(),
            video: false,
            current: Some("2016-11-28 10:00:00".into()),
            create: Some("2000-09-19 00:33:35".into()),
            original: Some("2000-09-19 00:33:35".into()),
            modify: Some("2016-11-28 18:48:43".into()),
        }
    }

    #[test]
    fn picks_toggle_and_manual_validates() {
        let mut d = Dates::default();
        d.start_scan(false);
        d.flagged(cand("a"));
        d.flagged(cand("b"));
        d.pick("a", DateKey::Create);
        let r = d.row(&d.rows[0].clone());
        assert!(
            r.options[0].chosen && r.options[1].chosen,
            "same value, both lit"
        );
        assert!(r.options[2].same_day);
        d.pick("a", DateKey::Create);
        assert!(d.choices.is_empty(), "second pick unchooses");
        assert!(d.manual("b", "22/09/2008").is_some());
        assert!(d.choices.is_empty());
        assert!(d.manual("b", "2008-09-19T01:33:03+00:00").is_none());
        assert!(d.row(&d.rows[1].clone()).manual_chosen);
        d.manual("b", "");
        assert!(
            d.choices.is_empty(),
            "clearing the text drops the typed choice"
        );
        d.pick_all(DateKey::Modify);
        assert_eq!(d.view(false).summary, "2 flagged, 2 chosen");
        d.scanning = false;
        assert!(d.view(false).can_apply);
        assert_eq!(d.take_choices().len(), 2);
    }
}
