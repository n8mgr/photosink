use super::{Kind, extension, is_media};
use crate::config::Config;
use crate::export::{Exporter, Summary, write_hashed};
use crate::photo::Asset;
use crate::state::State;
use crate::{Event, Progress};
use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

struct Index {
    assets: HashMap<String, Asset>,
    entries: HashMap<PathBuf, (String, Kind)>,
    pending: HashMap<String, usize>,
}

struct Import<'a> {
    cfg: &'a Config,
    idx: Index,
    ex: Exporter<'a>,
    seen: HashMap<String, usize>,
    filtered: HashSet<String>,
}

impl Import<'_> {
    /// Exports one indexed file, or plans it when `data` is None. The asset is
    /// reported and its sidecar written after its last file.
    fn entry(&mut self, rel: &Path, data: Option<&mut dyn Read>) {
        let Some((uuid, kind)) = self.idx.entries.get(rel) else {
            return;
        };
        let Some(asset) = self.idx.assets.get(uuid) else {
            return;
        };
        if !self.cfg.filter.matches(asset) {
            if self.filtered.insert(uuid.clone()) {
                self.ex.summary.filtered += 1;
            }
            return;
        }
        let total = self.idx.assets.len();
        let next = self.seen.len() + 1;
        let n = *self.seen.entry(uuid.clone()).or_insert(next);
        self.ex.set_pos(n, total);
        let ex = &mut self.ex;
        let pending = &mut self.idx.pending;
        let step = || -> Result<()> {
            if kind.wanted(self.cfg) {
                if ex.exported(uuid, *kind)? {
                    ex.summary.skipped += 1;
                } else if let Some(r) = data {
                    let tmp = ex.temp_path(uuid, *kind)?;
                    let hash = write_hashed(r, &tmp)?;
                    ex.place(asset, *kind, &name_of(rel), &tmp, &hash)?;
                } else {
                    ex.plan(asset, *kind, &name_of(rel))?;
                }
            }
            let left = pending.get_mut(uuid).unwrap();
            *left -= 1;
            if *left == 0 {
                ex.emit(Event::Asset {
                    n,
                    total,
                    name: asset.filename.clone(),
                    source_id: None,
                });
                ex.sidecar(asset)?;
            }
            Ok(())
        };
        if let Err(e) = step() {
            ex.fail(&asset.filename, format!("{e:#}"));
        }
    }
}

fn is_archive(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".zip") || name.ends_with(".tgz") || name.ends_with(".tar.gz")
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn safe_rel(name: &str) -> PathBuf {
    Path::new(name)
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn walk_dir(
    dir: &Path,
    prefix: &Path,
    f: &mut dyn FnMut(PathBuf, &mut dyn Read) -> Result<()>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let entry = entry?;
        let rel = prefix.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            walk_dir(&entry.path(), &rel, f)?;
        } else if !is_archive(&rel) {
            f(rel, &mut std::fs::File::open(entry.path())?)?;
        }
    }
    Ok(())
}

fn walk(path: &Path, mut f: impl FnMut(PathBuf, &mut dyn Read) -> Result<()>) -> Result<()> {
    if path.is_dir() {
        return walk_dir(path, Path::new(""), &mut f);
    }
    let file = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    if path.extension().is_some_and(|e| e == "zip") {
        let mut zip = zip::ZipArchive::new(file)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            if entry.is_file() {
                let rel = safe_rel(&entry.name()?);
                f(rel, &mut entry)?;
            }
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        for entry in tar.entries()? {
            let mut entry = entry?;
            if entry.header().entry_type().is_file() {
                let rel = safe_rel(&entry.path()?.to_string_lossy());
                f(rel, &mut entry)?;
            }
        }
    }
    Ok(())
}

fn split_paren(s: &str) -> (&str, &str) {
    if let Some(i) = s.rfind('(')
        && s.ends_with(')')
        && i + 2 < s.len()
        && s[i + 1..s.len() - 1].bytes().all(|b| b.is_ascii_digit())
    {
        return s.split_at(i);
    }
    (s, "")
}

fn media_key(media: &str) -> (String, &str) {
    let (stem, ext) = media.rsplit_once('.').unwrap_or((media, ""));
    let (base, n) = split_paren(stem);
    let key = if ext.is_empty() {
        base.to_owned()
    } else {
        format!("{base}.{ext}")
    };
    (key, n)
}

fn sidecar_matches(json: &str, media: &str) -> bool {
    let Some(key) = json.strip_suffix(".json") else {
        return false;
    };
    let (k, kn) = split_paren(key);
    let (m, mn) = media_key(media);
    kn == mn && (k == m || k.starts_with(&format!("{m}.")) || (k.len() >= 46 && m.starts_with(k)))
}

fn asset(uuid: String, filename: String, album: Option<String>, meta: &Value) -> Asset {
    let text = |k: &str| {
        meta[k]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let geo = ["geoData", "geoDataExif"].iter().find_map(|k| {
        let (lat, lon) = (
            meta[k]["latitude"].as_f64()?,
            meta[k]["longitude"].as_f64()?,
        );
        (lat != 0.0 || lon != 0.0).then_some((lat, lon))
    });
    let epoch = |v: &Value| -> Option<DateTime<FixedOffset>> {
        let t = v.as_str()?.parse().ok()?;
        Utc.timestamp_opt(t, 0).single().map(|t| t.fixed_offset())
    };
    let mut a = Asset {
        uuid,
        video: super::is_video(&filename),
        title: text("title").filter(|t| *t != filename),
        description: text("description"),
        taken: epoch(&meta["photoTakenTime"]["timestamp"]),
        uploaded: epoch(&meta["creationTime"]["timestamp"]),
        latitude: geo.map(|g| g.0),
        longitude: geo.map(|g| g.1),
        favorite: meta["favorited"].as_bool().unwrap_or(false),
        albums: album.into_iter().collect(),
        people: meta["people"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| p["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        filename,
        ..Default::default()
    };
    a.fill_place();
    a
}

fn index(sources: &[PathBuf], progress: Progress) -> Result<Index> {
    let (mut jsons, mut media) = (HashMap::new(), Vec::new());
    for s in sources {
        progress(Event::Listing {
            path: s.display().to_string(),
        });
        walk(s, |rel, r| {
            let name = name_of(&rel);
            if extension(&name) == "json" {
                let mut buf = Vec::new();
                r.read_to_end(&mut buf)?;
                jsons.insert(rel, buf);
            } else if is_media(&name) {
                media.push(rel);
            }
            Ok(())
        })?;
    }
    let mut dirs: HashMap<&Path, Vec<String>> = HashMap::new();
    for rel in &media {
        dirs.entry(rel.parent().unwrap_or(Path::new("")))
            .or_default()
            .push(name_of(rel));
    }
    let mut idx = Index {
        assets: HashMap::new(),
        entries: HashMap::new(),
        pending: HashMap::new(),
    };
    for (dir, names) in dirs {
        let album = dir
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.starts_with("Photos from "))
            .map(str::to_owned);
        let uuid_of = |name: &str| {
            blake3::hash(dir.join(name).to_string_lossy().as_bytes()).to_hex()[..16].to_owned()
        };
        let sidecars: Vec<&PathBuf> = jsons
            .keys()
            .filter(|j| j.parent().unwrap_or(Path::new("")) == dir)
            .collect();
        for name in &names {
            let (stem, e) = name.rsplit_once('.').unwrap_or((name, ""));
            let (uuid, kind) = match stem.strip_suffix("-edited") {
                Some(b) if names.contains(&format!("{b}.{e}")) => {
                    (uuid_of(&format!("{b}.{e}")), Kind::Edited)
                }
                _ => (uuid_of(name), Kind::Original),
            };
            if kind == Kind::Original {
                let meta = sidecars
                    .iter()
                    .find(|j| sidecar_matches(&name_of(j), name))
                    .and_then(|j| serde_json::from_slice(&jsons[*j]).ok())
                    .unwrap_or(Value::Null);
                idx.assets.insert(
                    uuid.clone(),
                    asset(uuid.clone(), name.clone(), album.clone(), &meta),
                );
            }
            *idx.pending.entry(uuid.clone()).or_default() += 1;
            idx.entries.insert(dir.join(name), (uuid, kind));
        }
    }
    Ok(idx)
}

pub(crate) fn import(
    path: &Path,
    cfg: &Config,
    state: &State,
    progress: Progress,
    dry: bool,
) -> Result<Summary> {
    let sources: Vec<PathBuf> = if path.is_dir() {
        let mut archives: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| Some(e.ok()?.path()))
            .filter(|p| is_archive(p))
            .collect();
        archives.sort();
        archives.insert(0, path.to_path_buf());
        archives
    } else {
        vec![path.to_path_buf()]
    };
    let idx = index(&sources, progress)?;
    progress(Event::Found {
        total: idx.assets.len(),
        place: path.display().to_string(),
    });
    let mut import = Import {
        cfg,
        idx,
        ex: Exporter::new(cfg, state, progress, dry),
        seen: HashMap::new(),
        filtered: HashSet::new(),
    };
    if dry {
        let mut rels: Vec<PathBuf> = import.idx.entries.keys().cloned().collect();
        rels.sort();
        for rel in rels {
            import.entry(&rel, None);
        }
    } else {
        for s in &sources {
            progress(Event::Reading {
                path: s.display().to_string(),
            });
            walk(s, |rel, r| {
                import.entry(&rel, Some(r));
                Ok(())
            })?;
        }
    }
    Ok(import.ex.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pairs_sidecars() {
        assert!(sidecar_matches("IMG_1.jpg.json", "IMG_1.jpg"));
        assert!(sidecar_matches(
            "IMG_1.jpg.supplemental-metadata.json",
            "IMG_1.jpg"
        ));
        assert!(sidecar_matches(
            "IMG_1.jpg.supplemental-metad.json",
            "IMG_1.jpg"
        ));
        assert!(sidecar_matches("IMG_1.jpg(1).json", "IMG_1(1).jpg"));
        assert!(sidecar_matches(
            "IMG_1.jpg.supplemental-metadata(1).json",
            "IMG_1(1).jpg"
        ));
        assert!(!sidecar_matches("IMG_1.jpg(1).json", "IMG_1.jpg"));
        assert!(!sidecar_matches("IMG_1.jpg.json", "IMG_1(1).jpg"));
        assert!(!sidecar_matches("IMG_10.jpg.json", "IMG_1.jpg"));
        let long = "a".repeat(60);
        assert!(sidecar_matches(
            &format!("{}.json", &long[..46]),
            &format!("{long}.jpg")
        ));
    }

    fn write_tree(dir: &Path) {
        let year = dir.join("Google Photos/Photos from 2019");
        let album = dir.join("Google Photos/Trip");
        std::fs::create_dir_all(&year).unwrap();
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(year.join("IMG_1.jpg"), b"orig").unwrap();
        std::fs::write(year.join("IMG_1-edited.jpg"), b"edit").unwrap();
        std::fs::write(
            year.join("IMG_1.jpg.supplemental-metadata.json"),
            r#"{"title":"IMG_1.jpg","description":"d","photoTakenTime":{"timestamp":"1556906879"},
                "geoData":{"latitude":48.8566,"longitude":2.3522},"people":[{"name":"Ann"}],"favorited":true}"#,
        )
        .unwrap();
        std::fs::write(album.join("VID_1.mp4"), b"vid").unwrap();
        std::fs::write(
            album.join("VID_1.mp4.json"),
            r#"{"creationTime":{"timestamp":"1262304000"}}"#,
        )
        .unwrap();
        std::fs::write(album.join("metadata.json"), "{}").unwrap();
    }

    fn write_archives(dir: &Path) {
        let inner = "Takeout/Google Photos/Photos from 2020/";
        let mut zip = zip::ZipWriter::new(std::fs::File::create(dir.join("a.zip")).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file(format!("{inner}IMG_2.jpg.json"), opts)
            .unwrap();
        zip.write_all(br#"{"description":"from zip","photoTakenTime":{"timestamp":"1577836800"}}"#)
            .unwrap();
        zip.start_file(format!("{inner}IMG_2-edited.jpg"), opts)
            .unwrap();
        zip.write_all(b"edit2").unwrap();
        zip.finish().unwrap();
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(dir.join("b.tgz")).unwrap(),
            flate2::Compression::default(),
        );
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(
            &mut header,
            format!("{inner}IMG_2.jpg"),
            b"orig2".as_slice(),
        )
        .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn indexes_tree_and_archives() {
        let dir = tempfile::tempdir().unwrap();
        write_tree(dir.path());
        write_archives(dir.path());
        let idx = index(
            &[
                dir.path().to_path_buf(),
                dir.path().join("a.zip"),
                dir.path().join("b.tgz"),
            ],
            &|_| {},
        )
        .unwrap();
        assert_eq!(idx.assets.len(), 3);
        let img = idx
            .assets
            .values()
            .find(|a| a.filename == "IMG_1.jpg")
            .unwrap();
        assert_eq!(img.description.as_deref(), Some("d"));
        assert_eq!(img.title, None);
        assert_eq!(
            img.taken.unwrap().format("%Y-%m-%d").to_string(),
            "2019-05-03"
        );
        assert_eq!(img.place.country.as_deref(), Some("FR"));
        assert_eq!(img.people, ["Ann"]);
        assert!(img.favorite && !img.video && img.albums.is_empty());
        assert_eq!(idx.pending[&img.uuid], 2);
        let vid = idx
            .assets
            .values()
            .find(|a| a.filename == "VID_1.mp4")
            .unwrap();
        assert!(vid.video);
        assert!(vid.taken.is_none(), "upload time is not a taken date");
        assert_eq!(vid.uploaded.unwrap().format("%Y").to_string(), "2010");
        assert_eq!(vid.albums, ["Trip"]);
        let img2 = idx
            .assets
            .values()
            .find(|a| a.filename == "IMG_2.jpg")
            .unwrap();
        assert_eq!(img2.description.as_deref(), Some("from zip"));
        assert_eq!(
            idx.entries[Path::new("Takeout/Google Photos/Photos from 2020/IMG_2-edited.jpg")],
            (img2.uuid.clone(), Kind::Edited)
        );
    }

    #[test]
    fn imports_and_skips_on_rerun() {
        let dir = tempfile::tempdir().unwrap();
        write_tree(dir.path());
        write_archives(dir.path());
        let out = dir.path().join("out");
        let cfg = Config::parse(&format!(
            "export_dir = {}\npath = \"{{take.year}}\"\nedits = true",
            toml::Value::String(out.to_string_lossy().into_owned())
        ))
        .unwrap();
        let state = State::open(&dir.path().join("state.sqlite")).unwrap();
        let s = import(dir.path(), &cfg, &state, &|e| eprintln!("{e}"), false).unwrap();
        assert_eq!((s.exported, s.errors), (5, 0));
        assert_eq!(std::fs::read(out.join("2019/IMG_1.jpg")).unwrap(), b"orig");
        assert_eq!(
            std::fs::read(out.join("2019/IMG_1_edited.jpg")).unwrap(),
            b"edit"
        );
        assert_eq!(std::fs::read(out.join("2010/VID_1.mp4")).unwrap(), b"vid");
        assert_eq!(std::fs::read(out.join("2020/IMG_2.jpg")).unwrap(), b"orig2");
        assert_eq!(
            std::fs::read(out.join("2020/IMG_2_edited.jpg")).unwrap(),
            b"edit2"
        );
        let sidecar =
            crate::photo::xmp::read(&std::fs::read_to_string(out.join("2020/IMG_2.xmp")).unwrap())
                .unwrap();
        assert_eq!(sidecar.description.as_deref(), Some("from zip"));
        assert!(!out.join(".tmp").exists());
        let s = import(dir.path(), &cfg, &state, &|e| eprintln!("{e}"), false).unwrap();
        assert_eq!((s.skipped, s.exported), (5, 0));
    }

    #[test]
    fn writes_sidecars_when_edits_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write_tree(dir.path());
        let out = dir.path().join("out");
        let cfg = Config::parse(&format!(
            "export_dir = {}\npath = \"{{take.year}}\"",
            toml::Value::String(out.to_string_lossy().into_owned())
        ))
        .unwrap();
        let state = State::open(&dir.path().join("state.sqlite")).unwrap();
        let s = import(dir.path(), &cfg, &state, &|e| eprintln!("{e}"), false).unwrap();
        assert_eq!((s.exported, s.errors), (2, 0));
        assert!(out.join("2019/IMG_1.xmp").exists());
        assert!(!out.join("2019/IMG_1_edited.jpg").exists());
    }
}
