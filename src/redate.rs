//! Review and correct taken dates after the fact. `scan` lists assets whose
//! sidecar date disagrees with what the file itself says; `apply` stores the
//! chosen date as an override, rewrites the sidecar and embedded packet, and
//! moves the asset to the folder the template now produces.

use crate::config::Config;
use crate::consolidate::{self, Consolidated};
use crate::photo::exif;
use crate::state::State;
use crate::{Event, Progress};
use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use std::path::Path;

pub const FORMAT: &str = "%Y-%m-%d %H:%M:%S";

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub uuid: String,
    pub stem: String,
    pub dir: String,
    pub video: bool,
    pub current: Option<NaiveDateTime>,
    pub create: Option<NaiveDateTime>,
    pub original: Option<NaiveDateTime>,
    pub modify: Option<NaiveDateTime>,
}

/// A new taken date for an asset, as wall-clock time read in the asset's
/// existing offset.
#[derive(Debug, Clone)]
pub struct Choice {
    pub uuid: String,
    pub taken: NaiveDateTime,
}

fn media_file(state: &State, uuid: &str) -> Result<Option<String>> {
    if let Some(p) = state.exported(uuid, crate::sources::Kind::Original)? {
        return Ok(Some(p));
    }
    Ok(state
        .exports_of(uuid)?
        .into_iter()
        .find(|(kind, _)| *kind != crate::sources::Kind::Adjustments)
        .map(|(_, p)| p))
}

fn differs(current: Option<NaiveDateTime>, dates: &exif::Dates) -> bool {
    let Some(current) = current else {
        return true;
    };
    match dates.create {
        Some(c) => c.date() != current.date(),
        None => [dates.original, dates.modify]
            .iter()
            .flatten()
            .any(|d| d.date() != current.date()),
    }
}

/// With `query`, lists every asset whose folder, stem or original filename
/// contains it, flagged or not; without, only those whose date disagrees with
/// the file. Each is reported as an `Event::Flagged`.
pub fn scan(cfg: &Config, state: &State, query: Option<&str>, progress: Progress) -> Result<()> {
    let assets = state.assets_under(&cfg.export_dir.to_string_lossy())?;
    let total = assets.len();
    let query = query
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(str::to_lowercase);
    progress(Event::Checking {
        total,
        query: query.clone(),
    });
    let mut flagged = 0;
    for (n, (uuid, dir, stem)) in assets.iter().enumerate() {
        if n % 100 == 99 {
            progress(Event::Checked {
                n: n + 1,
                total,
                flagged,
            });
        }
        let Some(asset) = crate::export::stored_asset(state, dir, stem, uuid)? else {
            continue;
        };
        if let Some(q) = &query
            && !(stem.to_lowercase().contains(q)
                || dir.to_lowercase().contains(q)
                || asset.filename.to_lowercase().contains(q))
        {
            continue;
        }
        let current = asset.taken.map(|t| t.naive_local());
        let Some(file) = media_file(state, uuid)? else {
            continue;
        };
        let dates = exif::read_metadata(Path::new(&file)).dates;
        if query.is_none()
            && (!differs(current, &dates) || (current.is_some() && dates == exif::Dates::default()))
        {
            continue;
        }
        flagged += 1;
        progress(Event::Flagged(Candidate {
            uuid: uuid.clone(),
            stem: stem.clone(),
            dir: Path::new(dir)
                .strip_prefix(&cfg.export_dir)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| dir.clone()),
            video: asset.video,
            current,
            create: dates.create,
            original: dates.original,
            modify: dates.modify,
        }));
    }
    progress(Event::Checked {
        n: total,
        total,
        flagged,
    });
    Ok(())
}

fn zoned(naive: NaiveDateTime, like: Option<DateTime<FixedOffset>>) -> DateTime<FixedOffset> {
    match like {
        Some(t) => t
            .offset()
            .from_local_datetime(&naive)
            .earliest()
            .unwrap_or_else(|| t.offset().from_utc_datetime(&naive)),
        None => Local
            .from_local_datetime(&naive)
            .earliest()
            .unwrap_or_else(|| Local.from_utc_datetime(&naive))
            .fixed_offset(),
    }
}

/// Redates asset `n` of `total`, at `dir`/`stem`.
fn redate(
    cfg: &Config,
    state: &State,
    c: &Choice,
    (dir, stem): (&str, &str),
    pos: (usize, usize),
    progress: Progress,
) -> Result<()> {
    let sidecar = Path::new(dir).join(format!("{stem}.xmp"));
    let mut asset = crate::export::stored_asset(state, dir, stem, &c.uuid)?
        .context("no metadata in sidecar or files")?;
    let new = zoned(c.taken, asset.taken);
    let old = asset.taken.map(|t| t.naive_local());
    asset.taken = Some(new);
    state.set(&format!("taken:{}", c.uuid), &new.to_rfc3339())?;
    let xmp = crate::photo::xmp::write(&asset);
    if cfg.sidecar || sidecar.exists() {
        std::fs::write(&sidecar, &xmp)?;
    }
    if cfg.embed {
        crate::export::embed_asset(state, &cfg.export_dir, &c.uuid, &xmp, pos, progress)?;
    }
    progress(Event::Redated {
        stem: stem.to_owned(),
        from: old,
        to: new.naive_local(),
    });
    Ok(())
}

/// Redates each chosen asset, reporting the ones that fail, then moves every
/// redated asset to the folder the template now produces.
pub fn apply(
    cfg: &Config,
    state: &State,
    choices: &[Choice],
    progress: Progress,
) -> Result<Consolidated> {
    let mut done = Vec::new();
    let total = choices.len();
    for (i, c) in choices.iter().enumerate() {
        let pos = (i + 1, total);
        let Some((dir, stem)) = state.location(&c.uuid)? else {
            progress(Event::error_at(
                pos.0,
                total,
                &c.uuid,
                "not in the state DB",
            ));
            continue;
        };
        match redate(cfg, state, c, (&dir, &stem), pos, progress) {
            Ok(()) => done.push((pos.0, (c.uuid.clone(), dir, stem))),
            Err(e) => progress(Event::error_at(pos.0, total, &stem, format!("{e:#}"))),
        }
    }
    consolidate::assets(cfg, state, &done, total, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, FORMAT).unwrap()
    }

    #[test]
    fn create_date_alone_decides_when_present() {
        let cur = Some(at("2016-11-28 10:00:00"));
        let d = exif::Dates {
            original: Some(at("2000-09-19 00:33:35")),
            create: Some(at("2000-09-19 00:33:35")),
            modify: Some(at("2008-09-22 18:48:43")),
        };
        assert!(differs(cur, &d));
        assert!(
            !differs(Some(at("2000-09-19 23:59:00")), &d),
            "same day as create is fine, whatever modify says"
        );
        assert!(differs(None, &exif::Dates::default()), "no date at all");
    }

    #[test]
    fn falls_back_to_original_and_modify_without_create() {
        let cur = Some(at("2016-11-28 10:00:00"));
        assert!(
            !differs(cur, &exif::Dates::default()),
            "nothing to compare against"
        );
        let modify_only = exif::Dates {
            modify: Some(at("2008-09-22 18:48:43")),
            ..exif::Dates::default()
        };
        assert!(differs(cur, &modify_only));
        assert!(!differs(Some(at("2008-09-22 01:00:00")), &modify_only));
    }

    /// Little-endian TIFF whose IFD0 holds only ModifyDate.
    fn tiff_dated(date: &str) -> Vec<u8> {
        let mut d = b"II*\0\x08\0\0\0".to_vec();
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&0x0132u16.to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&20u32.to_le_bytes());
        d.extend_from_slice(&26u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(date.as_bytes());
        d.push(0);
        d
    }

    struct Fixture {
        root: tempfile::TempDir,
        cfg: Config,
        state: State,
    }

    impl Fixture {
        fn new() -> Fixture {
            let root = tempfile::tempdir().unwrap();
            let out = root.path().join("out");
            let cfg = Config::parse(&format!(
                "export_dir = \"{}\"\npath = \"{{take.year}}\"\nembed = false",
                out.display()
            ))
            .unwrap();
            let state = State::open(&root.path().join("state.sqlite")).unwrap();
            Fixture { root, cfg, state }
        }

        fn dir(&self, year: &str) -> std::path::PathBuf {
            self.root.path().join("out").join(year)
        }

        /// An asset taken 2016-11-28 in `2016/`, its file holding `bytes`.
        fn seed(&self, uuid: &str, stem: &str, bytes: &[u8], sidecar: bool) {
            let dir = self.dir("2016");
            std::fs::create_dir_all(&dir).unwrap();
            let dir_s = dir.to_string_lossy();
            assert!(self.state.claim_stem(uuid, &dir_s, stem).unwrap());
            let file = dir.join(format!("{stem}.tif"));
            std::fs::write(&file, bytes).unwrap();
            self.state
                .record(
                    uuid,
                    crate::sources::Kind::Original,
                    &format!("h{uuid}"),
                    &file.to_string_lossy(),
                )
                .unwrap();
            if sidecar {
                let asset = crate::photo::Asset {
                    uuid: uuid.into(),
                    filename: format!("{stem}.tif"),
                    taken: DateTime::parse_from_rfc3339("2016-11-28T10:00:00+00:00").ok(),
                    ..Default::default()
                };
                std::fs::write(
                    dir.join(format!("{stem}.xmp")),
                    crate::photo::xmp::write(&asset),
                )
                .unwrap();
            }
        }
    }

    fn collect(f: impl FnOnce(Progress)) -> Vec<Event> {
        let events = std::sync::Mutex::new(Vec::new());
        f(&|e| events.lock().unwrap().push(e));
        events.into_inner().unwrap()
    }

    fn flagged(events: &[Event]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Flagged(c) => Some(c.uuid.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn scan_flags_disagreeing_files_and_query_lists_any_match() {
        let f = Fixture::new();
        f.seed("A", "IMG_1", &tiff_dated("2008:09:22 18:48:43"), true);
        f.seed("B", "IMG_2", &tiff_dated("2016:11:28 08:00:00"), true);
        f.seed("C", "IMG_3", b"no exif", true);
        let events = collect(|p| scan(&f.cfg, &f.state, None, p).unwrap());
        assert_eq!(flagged(&events), ["A"]);
        match events.iter().find(|e| matches!(e, Event::Flagged(_))) {
            Some(Event::Flagged(c)) => {
                assert_eq!(c.dir, "2016");
                assert_eq!(c.current, Some(at("2016-11-28 10:00:00")));
                assert_eq!(c.modify, Some(at("2008-09-22 18:48:43")));
            }
            _ => unreachable!(),
        }
        let events = collect(|p| scan(&f.cfg, &f.state, Some(" img_3 "), p).unwrap());
        assert_eq!(
            flagged(&events),
            ["C"],
            "a query lists matches unflagged too"
        );
    }

    #[test]
    fn apply_skips_failures_and_still_moves_the_rest() {
        let f = Fixture::new();
        f.seed("A", "IMG_1", b"a", true);
        f.seed("B", "IMG_2", b"b", false);
        f.seed("C", "IMG_3", b"c", true);
        f.seed("D", "IMG_4", b"d", true);
        let choice = |uuid: &str, taken: &str| Choice {
            uuid: uuid.into(),
            taken: at(taken),
        };
        let choices = [
            choice("A", "2008-09-22 00:00:00"),
            choice("B", "2008-09-22 00:00:00"),
            choice("Z", "2008-09-22 00:00:00"),
            choice("D", "2009-01-01 12:00:00"),
        ];
        let mut moved = None;
        let events = collect(|p| moved = Some(apply(&f.cfg, &f.state, &choices, p).unwrap()));
        let errors: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Error {
                    n: Some(n),
                    subject,
                    ..
                } => Some((*n, subject.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(errors, [(2, "IMG_2"), (3, "Z")]);
        let moves: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Moved { n, stem, .. } => Some((*n, stem.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(moves, [(1, "IMG_1"), (4, "IMG_4")], "numbered as chosen");
        assert_eq!(moved.unwrap().moved, 2);
        assert!(f.dir("2008").join("IMG_1.tif").exists());
        assert!(f.dir("2009").join("IMG_4.xmp").exists());
        assert!(f.dir("2016").join("IMG_2.tif").exists());
        assert!(f.dir("2016").join("IMG_3.tif").exists());
        assert!(f.state.get("taken:C").unwrap().is_none());
    }

    #[test]
    fn choices_keep_the_asset_offset() {
        let like = DateTime::parse_from_rfc3339("2016-11-28T10:00:00+01:00").unwrap();
        let z = zoned(at("2008-09-22 18:48:43"), Some(like));
        assert_eq!(z.to_rfc3339(), "2008-09-22T18:48:43+01:00");
        assert_eq!(
            zoned(at("2008-09-22 18:48:43"), None).naive_local(),
            at("2008-09-22 18:48:43")
        );
    }
}
