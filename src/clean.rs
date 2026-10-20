//! Removes what no longer belongs: rows whose file is gone (and the sidecar
//! and folder claim of an asset left with no files, so a later run exports it
//! again), and optionally media files the state does not reference.

use crate::config::Config;
use crate::state::State;
use crate::{Event, Progress};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Cleaned {
    pub rows: usize,
    pub dead_rows: usize,
    pub removed_assets: usize,
    pub removed_sidecars: usize,
    pub orphan_files: usize,
    pub removed_files: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// A state row whose file is not on disk.
    Missing,
    /// The sidecar of an asset left with no files.
    Abandoned,
    /// A sidecar no live asset claims.
    Unclaimed,
    /// A media file no state row references.
    Unreferenced,
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Reason::Missing => "file missing, row forgotten",
            Reason::Abandoned => "sidecar of an asset with no files",
            Reason::Unclaimed => "sidecar without an asset",
            Reason::Unreferenced => "not referenced by any asset",
        })
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn is_sidecar(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xmp"))
}

fn rel(cfg: &Config, p: &Path) -> String {
    p.strip_prefix(&cfg.export_dir)
        .map(|r| r.display().to_string())
        .unwrap_or_else(|_| p.display().to_string())
}

fn remove(path: &Path, dry: bool) -> Result<()> {
    if !dry {
        std::fs::remove_file(path).with_context(|| path.display().to_string())?;
    }
    Ok(())
}

/// With `orphans`, media files no row references are deleted too. With
/// `dry`, everything is reported and nothing is changed.
pub fn run(
    cfg: &Config,
    state: &State,
    orphans: bool,
    progress: Progress,
    dry: bool,
) -> Result<Cleaned> {
    let mut out = Cleaned::default();
    let mut files = Vec::new();
    walk(&cfg.export_dir, &mut files)?;
    let (sidecars, media): (Vec<PathBuf>, Vec<PathBuf>) =
        files.into_iter().partition(|p| is_sidecar(p));
    progress(Event::Inventory {
        files: media.len(),
        sidecars: sidecars.len(),
        dir: cfg.export_dir.display().to_string(),
    });
    let on_disk: HashSet<&Path> = media.iter().map(PathBuf::as_path).collect();

    let rows = state.all_exports()?;
    out.rows = rows.len();
    let mut alive: HashMap<String, usize> = HashMap::new();
    let mut referenced: HashSet<PathBuf> = HashSet::new();
    let mut dead = Vec::new();
    for row in &rows {
        if on_disk.contains(Path::new(&row.path)) {
            *alive.entry(row.uuid.clone()).or_default() += 1;
            referenced.insert(PathBuf::from(&row.path));
        } else {
            dead.push(row);
        }
    }

    let mut seen = HashSet::new();
    let mut handled: HashSet<PathBuf> = HashSet::new();
    for row in dead {
        out.dead_rows += 1;
        if !dry {
            state.forget_export(&row.uuid, row.kind)?;
        }
        progress(Event::Clean {
            path: rel(cfg, Path::new(&row.path)),
            reason: Reason::Missing,
            removed: !dry,
        });
        if alive.contains_key(&row.uuid) || !seen.insert(row.uuid.clone()) {
            continue;
        }
        if let Some((dir, stem)) = state.location(&row.uuid)? {
            let sidecar = Path::new(&dir).join(format!("{stem}.xmp"));
            if sidecar.exists() {
                remove(&sidecar, dry)?;
                handled.insert(sidecar.clone());
                out.removed_sidecars += 1;
                progress(Event::Clean {
                    path: rel(cfg, &sidecar),
                    reason: Reason::Abandoned,
                    removed: !dry,
                });
            }
            if !dry {
                state.forget_asset(&row.uuid)?;
            }
            out.removed_assets += 1;
        }
    }

    for p in &sidecars {
        let dir = p
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let claimed = match state.stem_owner(&dir, stem)? {
            Some(uuid) => alive.contains_key(&uuid),
            None => false,
        };
        if !claimed && !handled.contains(p) {
            remove(p, dry)?;
            out.removed_sidecars += 1;
            progress(Event::Clean {
                path: rel(cfg, p),
                reason: Reason::Unclaimed,
                removed: !dry,
            });
        }
    }

    for p in &media {
        if referenced.contains(p) {
            continue;
        }
        out.orphan_files += 1;
        if orphans {
            remove(p, dry)?;
            out.removed_files += 1;
        }
        progress(Event::Clean {
            path: rel(cfg, p),
            reason: Reason::Unreferenced,
            removed: orphans && !dry,
        });
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photo::Asset;
    use crate::sources::Kind;

    fn seed(state: &State, dir: &Path, uuid: &str, stem: &str, name: &str, bytes: &[u8]) -> String {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        state
            .claim_stem(uuid, &dir.to_string_lossy(), stem)
            .unwrap();
        let hash = blake3::hash(bytes).to_hex().to_string();
        state
            .record(uuid, Kind::Original, &hash, &path.to_string_lossy())
            .unwrap();
        let asset = Asset {
            uuid: uuid.into(),
            filename: name.into(),
            ..Default::default()
        };
        std::fs::write(
            dir.join(format!("{stem}.xmp")),
            crate::photo::xmp::write(&asset),
        )
        .unwrap();
        hash
    }

    #[test]
    fn removes_dead_rows_sidecars_and_optionally_orphans() {
        let root = tempfile::tempdir().unwrap();
        let out_dir = root.path().join("out");
        let cfg = Config::parse(&format!("export_dir = \"{}\"", out_dir.display())).unwrap();
        let state = State::open(&root.path().join("state.sqlite")).unwrap();
        let a = out_dir.join("2019/05");
        seed(&state, &a, "A", "IMG_1", "IMG_1.HEIC", b"keep");
        // B: recorded file vanished, sidecar left behind.
        seed(&state, &a, "B", "IMG_2", "IMG_2.CR3", b"gone");
        std::fs::remove_file(a.join("IMG_2.CR3")).unwrap();
        // D: duplicate-only row pointing at an old path that no longer exists.
        state
            .record(
                "D",
                Kind::Original,
                "h",
                &out_dir.join("2019-05-03/IMG_1.HEIC").to_string_lossy(),
            )
            .unwrap();
        std::fs::write(a.join("stray.PNG"), b"stray").unwrap();
        std::fs::write(a.join("lonely.xmp"), b"<x/>").unwrap();

        let c = run(&cfg, &state, false, &|e| eprintln!("{e}"), true).unwrap();
        assert_eq!(
            (
                c.dead_rows,
                c.removed_assets,
                c.removed_sidecars,
                c.orphan_files,
                c.removed_files
            ),
            (2, 1, 2, 1, 0)
        );
        assert!(
            a.join("IMG_2.xmp").exists() && a.join("stray.PNG").exists(),
            "dry run deletes nothing"
        );
        assert!(state.exported("B", Kind::Original).unwrap().is_some());

        let c = run(&cfg, &state, false, &|e| eprintln!("{e}"), false).unwrap();
        assert_eq!(
            (
                c.dead_rows,
                c.removed_assets,
                c.removed_sidecars,
                c.orphan_files,
                c.removed_files
            ),
            (2, 1, 2, 1, 0)
        );
        assert!(!a.join("IMG_2.xmp").exists() && !a.join("lonely.xmp").exists());
        assert!(a.join("stray.PNG").exists(), "orphans kept unless asked");
        assert_eq!(state.exported("B", Kind::Original).unwrap(), None);
        assert_eq!(state.stem("B").unwrap(), None, "B can be exported again");
        assert_eq!(state.exported("D", Kind::Original).unwrap(), None);
        assert!(a.join("IMG_1.HEIC").exists() && a.join("IMG_1.xmp").exists());

        let c = run(&cfg, &state, true, &|e| eprintln!("{e}"), false).unwrap();
        assert_eq!((c.dead_rows, c.orphan_files, c.removed_files), (0, 1, 1));
        assert!(!a.join("stray.PNG").exists());
    }
}
