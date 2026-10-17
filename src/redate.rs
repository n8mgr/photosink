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
use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FORMAT: &str = "%Y-%m-%d %H:%M:%S";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Candidate {
    pub uuid: String,
    pub stem: String,
    pub dir: String,
    pub video: bool,
    pub current: Option<String>,
    pub create: Option<String>,
    pub original: Option<String>,
    pub modify: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub uuid: String,
    pub taken: String,
}

fn fmt(t: NaiveDateTime) -> String {
    t.format(FORMAT).to_string()
}

enum Parsed {
    /// Carries its own offset, used as given.
    Zoned(DateTime<FixedOffset>),
    /// Wall-clock only; read in the asset's existing offset.
    Naive(NaiveDateTime),
}

fn parse(s: &str) -> Result<Parsed> {
    let s = s.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(Parsed::Zoned(t));
    }
    NaiveDateTime::parse_from_str(s, FORMAT)
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap())
        })
        .map(Parsed::Naive)
        .with_context(|| format!("date {s:?} is not YYYY-MM-DD, YYYY-MM-DD HH:MM:SS or RFC 3339"))
}

/// Checks that `s` is a date `apply` accepts.
pub fn check(s: &str) -> Result<()> {
    parse(s).map(|_| ())
}

fn media_file(state: &State, uuid: &str) -> Result<Option<String>> {
    if let Some(p) = state.exported(uuid, "original")? {
        return Ok(Some(p));
    }
    Ok(state
        .exports_of(uuid)?
        .into_iter()
        .find(|(kind, _)| kind != "adjustments")
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
/// the file.
pub fn scan(
    cfg: &Config,
    state: &State,
    query: Option<&str>,
    progress: Progress,
) -> Result<Vec<Candidate>> {
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
    let mut out = Vec::new();
    for (n, (uuid, dir, stem)) in assets.iter().enumerate() {
        if n % 100 == 99 {
            progress(Event::Checked {
                n: n + 1,
                total,
                flagged: out.len(),
            });
        }
        if let Some(q) = &query
            && !(stem.to_lowercase().contains(q) || dir.to_lowercase().contains(q))
            && !crate::export::stored_asset(state, dir, stem, uuid)?
                .is_some_and(|a| a.filename.to_lowercase().contains(q))
        {
            continue;
        }
        let Some(asset) = crate::export::stored_asset(state, dir, stem, uuid)? else {
            continue;
        };
        // Same wall-clock reading the path template uses, so a UTC-stamped source
        // date is compared to the file's naive EXIF time without a zone shift.
        let current = asset.taken.map(|t| t.naive_local());
        let Some(file) = media_file(state, uuid)? else {
            continue;
        };
        let dates = exif::dates(Path::new(&file));
        if query.is_none()
            && (!differs(current, &dates) || (current.is_some() && dates == exif::Dates::default()))
        {
            continue;
        }
        let c = Candidate {
            uuid: uuid.clone(),
            stem: stem.clone(),
            dir: Path::new(dir)
                .strip_prefix(&cfg.export_dir)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| dir.clone()),
            video: asset.video,
            current: current.map(fmt),
            create: dates.create.map(fmt),
            original: dates.original.map(fmt),
            modify: dates.modify.map(fmt),
        };
        progress(Event::Flagged(c.clone()));
        out.push(c);
    }
    progress(Event::Checked {
        n: total,
        total,
        flagged: out.len(),
    });
    Ok(out)
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

pub fn apply(
    cfg: &Config,
    state: &State,
    choices: &[Choice],
    progress: Progress,
) -> Result<Consolidated> {
    let mut done = Vec::new();
    let total = choices.len();
    for (i, c) in choices.iter().enumerate() {
        let Some((dir, stem)) = state.location(&c.uuid)? else {
            progress(Event::error(&c.uuid, "not in the state DB"));
            continue;
        };
        let sidecar = Path::new(&dir).join(format!("{stem}.xmp"));
        let mut asset = crate::export::stored_asset(state, &dir, &stem, &c.uuid)?
            .with_context(|| format!("{stem}: no metadata in sidecar or files"))?;
        let new = match parse(&c.taken)? {
            Parsed::Zoned(t) => t,
            Parsed::Naive(n) => zoned(n, asset.taken),
        };
        let old = asset.taken.map(|t| t.format(FORMAT).to_string());
        asset.taken = Some(new);
        state.set(&format!("taken:{}", c.uuid), &new.to_rfc3339())?;
        let xmp = crate::photo::xmp::write(&asset);
        if cfg.sidecar || sidecar.exists() {
            std::fs::write(&sidecar, &xmp)?;
        }
        if cfg.embed {
            crate::export::embed_asset(
                state,
                &cfg.export_dir,
                &c.uuid,
                &xmp,
                (i + 1, total),
                progress,
            )?;
        }
        progress(Event::Redated {
            stem: stem.clone(),
            from: old.unwrap_or_else(|| "none".into()),
            to: new.format(FORMAT).to_string(),
        });
        done.push(c.uuid.clone());
    }
    consolidate::assets(cfg, state, &done, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, FORMAT).unwrap()
    }

    #[test]
    fn flags_only_disagreement_with_create_date() {
        let cur = Some(at("2016-11-28 10:00:00"));
        let d = exif::Dates {
            original: Some(at("2000-09-19 00:33:35")),
            create: Some(at("2000-09-19 00:33:35")),
            modify: Some(at("2008-09-22 18:48:43")),
        };
        assert!(differs(cur, &d));
        assert!(
            !differs(Some(at("2000-09-19 23:59:00")), &d),
            "same day as create is fine"
        );
        let no_create = exif::Dates {
            create: None,
            ..exif::Dates::default()
        };
        assert!(!differs(cur, &no_create), "nothing to compare against");
        let modify_only = exif::Dates {
            modify: Some(at("2008-09-22 18:48:43")),
            ..exif::Dates::default()
        };
        assert!(differs(cur, &modify_only));
        assert!(differs(None, &exif::Dates::default()));
    }

    #[test]
    fn current_is_read_in_the_assets_own_offset() {
        let utc = DateTime::parse_from_rfc3339("2000-09-19T00:33:23+00:00").unwrap();
        let d = exif::Dates {
            create: Some(at("2000-09-19 00:33:23")),
            ..exif::Dates::default()
        };
        assert!(!differs(Some(utc.naive_local()), &d));
    }

    fn naive(s: &str) -> NaiveDateTime {
        match parse(s).unwrap() {
            Parsed::Naive(n) => n,
            Parsed::Zoned(t) => panic!("{s} parsed with offset {t}"),
        }
    }

    #[test]
    fn parses_choices_and_keeps_offset() {
        assert_eq!(naive("2008-09-22 18:48:43"), at("2008-09-22 18:48:43"));
        assert_eq!(naive("2008-09-22T18:48:43"), at("2008-09-22 18:48:43"));
        assert_eq!(naive(" 2008-09-22 "), at("2008-09-22 00:00:00"));
        match parse("2008-09-19T01:33:03+00:00").unwrap() {
            Parsed::Zoned(t) => assert_eq!(t.to_rfc3339(), "2008-09-19T01:33:03+00:00"),
            Parsed::Naive(_) => panic!("offset dropped"),
        }
        assert!(parse("22/09/2008").is_err());
        let like = DateTime::parse_from_rfc3339("2016-11-28T10:00:00+01:00").unwrap();
        let z = zoned(at("2008-09-22 18:48:43"), Some(like));
        assert_eq!(z.to_rfc3339(), "2008-09-22T18:48:43+01:00");
        assert_eq!(
            zoned(at("2008-09-22 18:48:43"), None).naive_local(),
            at("2008-09-22 18:48:43")
        );
    }
}
