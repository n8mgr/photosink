use crate::config::Config;
use crate::state::State;
use crate::template;
use crate::{Event, Progress};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Default, Debug, Clone)]
pub struct Consolidated {
    pub moved: usize,
    pub restemmed: usize,
    pub unchanged: usize,
    pub missing: usize,
}

struct Pass<'a> {
    cfg: &'a Config,
    state: &'a State,
    progress: Progress<'a>,
    dry: bool,
    out: Consolidated,
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    anyhow::ensure!(!to.exists(), "{} exists", to.display());
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to).with_context(|| format!("{} -> {}", from.display(), to.display()))
}

fn restem(name: &str, old: &str, new: &str) -> String {
    match name.strip_prefix(old) {
        Some(rest) if rest.is_empty() || rest.starts_with(['.', '_']) => format!("{new}{rest}"),
        _ => name.to_owned(),
    }
}

fn pass<'a>(cfg: &'a Config, state: &'a State, progress: Progress<'a>, dry: bool) -> Pass<'a> {
    Pass {
        cfg,
        state,
        progress,
        dry,
        out: Consolidated::default(),
    }
}

fn prune(mut dir: PathBuf, root: &Path) {
    while dir.starts_with(root) && dir != root && std::fs::remove_dir(&dir).is_ok() {
        dir.pop();
    }
}

/// The stem `uuid` can use in `new_dir`: `wanted`, or `wanted` with a uuid
/// suffix when another asset holds it there.
fn free_stem(state: &State, uuid: &str, new_dir: &str, wanted: &str) -> Result<String> {
    Ok(match state.stem_owner(new_dir, wanted)? {
        Some(owner) if owner != uuid => {
            let candidate = format!("{wanted}_{}", &uuid[..uuid.len().min(8)]);
            if state
                .stem_owner(new_dir, &candidate)?
                .is_some_and(|o| o != uuid)
            {
                bail!("stem {wanted} and {candidate} both taken in {new_dir}");
            }
            candidate
        }
        _ => wanted.to_owned(),
    })
}

fn relocate(
    state: &State,
    uuid: &str,
    dir: &Path,
    stem: &str,
    new_dir: &Path,
    new_stem: &str,
) -> Result<()> {
    let new_dir_s = new_dir.to_string_lossy().into_owned();
    let mut paths = Vec::new();
    for (kind, path) in state.exports_of(uuid)? {
        let p = Path::new(&path);
        if !p.starts_with(dir) {
            continue;
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let dest = new_dir.join(restem(name, stem, new_stem));
        if p.exists() {
            move_file(p, &dest)?;
        }
        paths.push((kind, dest.to_string_lossy().into_owned()));
    }
    let sidecar = dir.join(format!("{stem}.xmp"));
    if sidecar.exists() {
        move_file(&sidecar, &new_dir.join(format!("{new_stem}.xmp")))?;
    }
    state.relocate(uuid, &new_dir_s, new_stem, &paths)?;
    Ok(())
}

impl Pass<'_> {
    /// Moves asset `n` of `total`, `uuid` now at `dir`/`stem`.
    fn one(&mut self, (n, total): (usize, usize), uuid: &str, dir: &str, stem: &str) -> Result<()> {
        let (cfg, state) = (self.cfg, self.state);
        let Some(asset) = crate::export::stored_asset(state, dir, stem, uuid)? else {
            self.out.missing += 1;
            (self.progress)(Event::error_at(n, total, stem, "no metadata, left as is"));
            return Ok(());
        };
        let dir = PathBuf::from(dir);
        let zoned = asset.zoned(cfg.date_zone);
        let new_dir = cfg
            .export_dir
            .join(template::render(&cfg.path, |k, i| zoned.field(k, i)));
        let wanted = crate::export::file_stem(&cfg.filename, &zoned);
        let new_stem = free_stem(state, uuid, &new_dir.to_string_lossy(), &wanted)
            .with_context(|| format!("{uuid} {stem}"))?;
        if new_dir == dir && new_stem == stem {
            self.out.unchanged += 1;
            return Ok(());
        }
        let restemmed = new_stem != stem;
        if !self.dry {
            relocate(state, uuid, &dir, stem, &new_dir, &new_stem)
                .with_context(|| format!("{uuid} {stem}"))?;
            prune(dir, &cfg.export_dir);
        }
        self.out.moved += 1;
        self.out.restemmed += restemmed as usize;
        (self.progress)(Event::Moved {
            n,
            total,
            stem: stem.to_owned(),
            dest: format!(
                "{}/{new_stem}",
                new_dir
                    .strip_prefix(&cfg.export_dir)
                    .unwrap_or(&new_dir)
                    .display()
            ),
        });
        Ok(())
    }

    fn all<'b>(
        mut self,
        assets: impl Iterator<Item = (usize, &'b (String, String, String))>,
        total: usize,
    ) -> Result<Consolidated> {
        for (n, (uuid, dir, stem)) in assets {
            self.one((n, total), uuid, dir, stem)?;
        }
        Ok(self.out)
    }
}

pub fn run(cfg: &Config, state: &State, progress: Progress, dry: bool) -> Result<Consolidated> {
    let assets = state.assets_under(&cfg.export_dir.to_string_lossy())?;
    progress(Event::Found {
        total: assets.len(),
        place: cfg.export_dir.display().to_string(),
    });
    let numbered = assets.iter().enumerate().map(|(i, a)| (i + 1, a));
    pass(cfg, state, progress, dry).all(numbered, assets.len())
}

/// Consolidates just the given `(uuid, dir, stem)` assets, each numbered
/// `n` of `total` as the caller numbered it.
pub fn assets(
    cfg: &Config,
    state: &State,
    assets: &[(usize, (String, String, String))],
    total: usize,
    progress: Progress,
) -> Result<Consolidated> {
    let numbered = assets.iter().map(|(n, a)| (*n, a));
    pass(cfg, state, progress, false).all(numbered, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photo::Asset;
    use crate::photo::geo::Place;

    #[test]
    fn restems_matching_names() {
        assert_eq!(restem("IMG_1.jpg", "IMG_1", "IMG_1_abcd"), "IMG_1_abcd.jpg");
        assert_eq!(
            restem("IMG_1_edited.jpg", "IMG_1", "IMG_1_abcd"),
            "IMG_1_abcd_edited.jpg"
        );
        assert_eq!(restem("IMG_10.jpg", "IMG_1", "X"), "IMG_10.jpg");
        assert_eq!(restem("other.jpg", "IMG_1", "X"), "other.jpg");
    }

    fn seed(state: &State, dir: &Path, uuid: &str, stem: &str, files: &[&str], asset: &Asset) {
        std::fs::create_dir_all(dir).unwrap();
        assert!(
            state
                .claim_stem(uuid, &dir.to_string_lossy(), stem)
                .unwrap()
        );
        for f in files {
            std::fs::write(dir.join(f), f).unwrap();
            let kind = if f.contains("_edited") {
                crate::sources::Kind::Edited
            } else {
                crate::sources::Kind::Original
            };
            state
                .record(
                    uuid,
                    kind,
                    &format!("h{uuid}{f}"),
                    &dir.join(f).to_string_lossy(),
                )
                .unwrap();
        }
        std::fs::write(
            dir.join(format!("{stem}.xmp")),
            crate::photo::xmp::write(asset),
        )
        .unwrap();
    }

    fn asset(uuid: &str, filename: &str, city: &str, name: &str) -> Asset {
        Asset {
            uuid: uuid.into(),
            filename: filename.into(),
            taken: chrono::DateTime::parse_from_rfc3339("2001-01-01T00:00:00Z").ok(),
            place: Place {
                name: Some(name.into()),
                city: Some(city.into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn relayouts_to_current_template() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        let state = State::open(&root.path().join("state.sqlite")).unwrap();
        let old = |s: &str| out.join("2001").join(s);
        seed(
            &state,
            &old("Paris"),
            "u",
            "IMG_1",
            &["IMG_1.jpg", "IMG_1_edited.jpg"],
            &asset("u", "IMG_1.jpg", "Paris", "Paris"),
        );
        seed(
            &state,
            &old("Paris 1er"),
            "v",
            "IMG_1",
            &["IMG_1.heic"],
            &asset("v", "IMG_1.heic", "Paris", "Paris 1er"),
        );
        seed(
            &state,
            &old("Lyon"),
            "w",
            "IMG_2",
            &["IMG_2.jpg"],
            &asset("w", "IMG_2.jpg", "Lyon", "Lyon"),
        );
        seed(
            &state,
            &old("Nowhere"),
            "x",
            "IMG_3",
            &["IMG_3.jpg"],
            &asset("x", "IMG_3.jpg", "Nowhere", "Nowhere"),
        );
        std::fs::remove_file(old("Nowhere").join("IMG_3.xmp")).unwrap();

        let cfg = Config::parse(&format!(
            "export_dir = \"{}\"\npath = \"{{take.year}}/{{place.city}}\"",
            out.display()
        ))
        .unwrap();
        let r = run(&cfg, &state, &|e| eprintln!("{e}"), false).unwrap();
        assert_eq!((r.moved, r.restemmed, r.unchanged, r.missing), (1, 1, 2, 1));
        let paris = old("Paris");
        assert!(paris.join("IMG_1.jpg").exists() && paris.join("IMG_1_edited.jpg").exists());
        assert!(paris.join("IMG_1_v.heic").exists() && paris.join("IMG_1_v.xmp").exists());
        assert!(!old("Paris 1er").exists());
        assert!(old("Lyon").join("IMG_2.jpg").exists());
        assert!(old("Nowhere").join("IMG_3.jpg").exists());
        assert_eq!(state.stem("v").unwrap().as_deref(), Some("IMG_1_v"));
        assert_eq!(
            state
                .exported("v", crate::sources::Kind::Original)
                .unwrap()
                .as_deref(),
            paris.join("IMG_1_v.heic").to_str()
        );

        let r = run(&cfg, &state, &|_| {}, false).unwrap();
        assert_eq!((r.moved, r.unchanged, r.missing), (0, 3, 1));
    }
}
