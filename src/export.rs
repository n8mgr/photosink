use crate::config::Config;
use crate::photo::embed::Embed;
use crate::photo::exif;
use crate::photo::{Asset, Camera};
use crate::sources::{Kind, Source};
use crate::state::{Placed, State};
use crate::template;
use crate::{Event, Progress};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};

#[derive(Default, Debug, Clone)]
pub struct Summary {
    pub exported: usize,
    pub duplicates: usize,
    pub skipped: usize,
    pub filtered: usize,
    pub errors: usize,
}

impl Summary {
    fn add(&mut self, o: Summary) {
        self.exported += o.exported;
        self.duplicates += o.duplicates;
        self.skipped += o.skipped;
        self.filtered += o.filtered;
        self.errors += o.errors;
    }
}

pub(crate) fn hash_file(path: &Path) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().to_hex().to_string())
}

pub(crate) fn write_hashed(mut src: impl Read, dest: &Path) -> Result<String> {
    let mut file = std::fs::File::create(dest)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            return Ok(hasher.finalize().to_hex().to_string());
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
    }
}

fn link_unique(tmp: &Path, dest: &Path, uuid: &str, hash: &str) -> Result<Placed> {
    let fallback = {
        let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let ext = dest
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        dest.with_file_name(format!("{stem}_{}{ext}", &uuid[..uuid.len().min(8)]))
    };
    for candidate in [dest.to_path_buf(), fallback] {
        match std::fs::hard_link(tmp, &candidate) {
            Ok(()) => {
                std::fs::remove_file(tmp)?;
                return Ok(Placed::New(candidate.to_string_lossy().into_owned()));
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                if hash_file(&candidate)? == hash {
                    std::fs::remove_file(tmp)?;
                    return Ok(Placed::Duplicate(candidate.to_string_lossy().into_owned()));
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("{} exists", dest.display())
}

/// The stem the filename template produces for the asset, falling back to the
/// original filename's stem when it renders empty.
pub(crate) fn file_stem(filename: &str, zoned: &Asset) -> String {
    let s = template::render(filename, |k, i| zoned.field(k, i)).replace('/', "-");
    if s.is_empty() {
        template::sanitize(zoned.stem())
    } else {
        s
    }
}

/// A free stem for the asset in `dir`: the template's, or that with a uuid
/// suffix. With `claim` the stem is recorded; without, two unexported assets
/// planning the same name in one folder both see it as free.
fn pick_stem(
    state: &State,
    cfg: &Config,
    zoned: &Asset,
    dir: &Path,
    claim: bool,
) -> Result<String> {
    let base = file_stem(&cfg.filename, zoned);
    let dir = dir.to_string_lossy();
    let uuid = &zoned.uuid;
    for stem in [
        base.clone(),
        format!("{base}_{}", &uuid[..8.min(uuid.len())]),
    ] {
        let free = if claim {
            state.claim_stem(uuid, &dir, &stem)?
        } else {
            state.stem_owner(&dir, &stem)?.is_none_or(|o| o == *uuid)
        };
        if free {
            return Ok(stem);
        }
    }
    anyhow::bail!("stem {base} taken in {dir}")
}

pub(crate) struct Exporter<'a> {
    cfg: &'a Config,
    state: &'a State,
    progress: Progress<'a>,
    location: Option<(String, PathBuf, String)>,
    pos: (usize, usize),
    dry: bool,
    pub(crate) summary: Summary,
}

impl<'a> Exporter<'a> {
    /// With `dry`, nothing is downloaded, written or recorded; planned
    /// placements are logged and counted as exported.
    pub(crate) fn new(
        cfg: &'a Config,
        state: &'a State,
        progress: Progress<'a>,
        dry: bool,
    ) -> Exporter<'a> {
        Exporter {
            cfg,
            state,
            progress,
            location: None,
            pos: (0, 0),
            dry,
            summary: Summary::default(),
        }
    }

    /// The asset index that detail events carry, so events from concurrent
    /// workers can be told apart.
    pub(crate) fn set_pos(&mut self, n: usize, total: usize) {
        self.pos = (n, total);
    }

    pub(crate) fn emit(&self, event: Event) {
        (self.progress)(event)
    }

    /// Reports an error on the current asset and counts it.
    pub(crate) fn fail(&mut self, subject: impl Into<String>, text: impl std::fmt::Display) {
        self.emit(Event::error_at(self.pos.0, self.pos.1, subject, text));
        self.summary.errors += 1;
    }

    fn location(&mut self, asset: &Asset) -> Result<(PathBuf, String)> {
        if let Some((uuid, dir, stem)) = &self.location
            && uuid == &asset.uuid
        {
            return Ok((dir.clone(), stem.clone()));
        }
        let (dir, stem) = match self.state.location(&asset.uuid)? {
            Some((dir, stem)) => (PathBuf::from(dir), stem),
            None => {
                let zoned = asset.zoned(self.cfg.date_zone);
                let dir = self
                    .cfg
                    .export_dir
                    .join(template::render(&self.cfg.path, |n, i| zoned.field(n, i)));
                if !self.dry {
                    std::fs::create_dir_all(&dir)?;
                }
                let stem = pick_stem(self.state, self.cfg, &zoned, &dir, !self.dry)?;
                (dir, stem)
            }
        };
        self.location = Some((asset.uuid.clone(), dir.clone(), stem.clone()));
        Ok((dir, stem))
    }

    pub(crate) fn exported(&self, uuid: &str, kind: Kind) -> Result<bool> {
        Ok(self.state.exported(uuid, kind)?.is_some())
    }

    pub(crate) fn temp_path(&self, uuid: &str, kind: Kind) -> Result<PathBuf> {
        let dir = self.cfg.export_dir.join(".tmp");
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join(format!("{uuid}.{}", kind.name())))
    }

    pub(crate) fn place(
        &mut self,
        asset: &Asset,
        kind: Kind,
        filename: &str,
        tmp: &Path,
        hash: &str,
    ) -> Result<()> {
        let asset = &self.resolve(asset, Some(tmp))?.0;
        let target = match self.state.duplicate(hash)? {
            Some(_) => None,
            None => {
                let (dir, stem) = self.location(asset)?;
                Some(dir.join(kind.file_name(&stem, filename)))
            }
        };
        let uuid = &asset.uuid;
        let placed = self.state.place(uuid, kind, hash, tmp, || {
            link_unique(tmp, target.as_deref().context("placed twice")?, uuid, hash)
        })?;
        match placed {
            Placed::New(p) => {
                self.summary.exported += 1;
                self.emit(Event::Placed {
                    n: self.pos.0,
                    total: self.pos.1,
                    file: filename.to_owned(),
                    dest: self.rel(&p),
                });
            }
            Placed::Duplicate(p) => {
                self.summary.duplicates += 1;
                self.emit(Event::Duplicate {
                    n: self.pos.0,
                    total: self.pos.1,
                    file: filename.to_owned(),
                    existing: self.rel(&p),
                });
            }
        }
        Ok(())
    }

    fn rel(&self, path: &str) -> String {
        Path::new(path)
            .strip_prefix(&self.cfg.export_dir)
            .map(|r| r.display().to_string())
            .unwrap_or_else(|_| path.to_owned())
    }

    /// Logs where a resource would land, for a dry run.
    pub(crate) fn plan(&mut self, asset: &Asset, kind: Kind, filename: &str) -> Result<()> {
        let asset = &self.resolve(asset, None)?.0;
        let (dir, stem) = self.location(asset)?;
        let dest = dir.join(kind.file_name(&stem, filename));
        self.summary.exported += 1;
        self.emit(Event::Placed {
            n: self.pos.0,
            total: self.pos.1,
            file: filename.to_owned(),
            dest: self.rel(&dest.to_string_lossy()),
        });
        Ok(())
    }

    pub(crate) fn sidecar(&mut self, asset: &Asset) -> Result<()> {
        if self.dry || self.state.location(&asset.uuid)?.is_none() {
            return Ok(());
        }
        let original = self.state.exported(&asset.uuid, Kind::Original)?;
        let (mut asset, meta) = self.resolve(asset, original.as_deref().map(Path::new))?;
        let (dir, stem) = self.location(&asset)?;
        let needs_exif = asset.latitude.is_none() || asset.camera == Camera::default();
        if let (true, Some(original)) = (needs_exif, original) {
            let meta = meta.unwrap_or_else(|| exif::read_metadata(Path::new(&original)));
            if let (None, Some((lat, lon))) = (asset.latitude, meta.gps) {
                asset.latitude = Some(lat);
                asset.longitude = Some(lon);
                asset.fill_place();
            }
            if asset.camera == Camera::default() {
                asset.camera = meta.camera;
            }
        }
        let xmp = crate::photo::xmp::write(&asset);
        if self.cfg.sidecar {
            std::fs::write(dir.join(format!("{stem}.xmp")), &xmp)?;
        }
        if self.cfg.embed {
            self.embed(&asset.uuid, &xmp)?;
        }
        Ok(())
    }

    fn embed(&self, uuid: &str, xmp: &str) -> Result<()> {
        embed_asset(
            self.state,
            &self.cfg.export_dir,
            uuid,
            xmp,
            self.pos,
            self.progress,
        )
    }

    /// The asset with its taken date settled: a date stored under `taken:<uuid>`
    /// (set by a redate or an earlier run) wins, then the source's own date, then
    /// what the file carries, then the upload time. Returns the file's
    /// metadata too when it had to be read.
    fn resolve(
        &self,
        asset: &Asset,
        file: Option<&Path>,
    ) -> Result<(Asset, Option<exif::Metadata>)> {
        let mut asset = asset.clone();
        let key = format!("taken:{}", asset.uuid);
        if let Some(t) = self.state.get(&key)? {
            asset.taken = chrono::DateTime::parse_from_rfc3339(&t).ok();
            return Ok((asset, None));
        }
        let file_first = self.cfg.date_source == crate::config::DateSource::File;
        let meta = file
            .filter(|_| file_first || asset.taken.is_none())
            .map(exif::read_metadata);
        match meta.as_ref().and_then(|m| m.dates.taken()) {
            Some(t) if file_first => asset.taken = Some(t),
            _ if asset.taken.is_some() => {}
            Some(t) => {
                if !self.dry {
                    self.state.set(&key, &t.to_rfc3339())?;
                }
                asset.taken = Some(t);
            }
            None => asset.taken = asset.uploaded,
        }
        Ok((asset, meta))
    }

    fn asset(&mut self, src: &dyn Source, asset: &Asset, local_id: &str) -> Result<()> {
        for r in src.resources(asset, local_id)? {
            if !r.kind.wanted(self.cfg) {
                continue;
            }
            if self.exported(&asset.uuid, r.kind)? {
                self.summary.skipped += 1;
                continue;
            }
            if self.dry {
                self.plan(asset, r.kind, &r.filename)?;
                continue;
            }
            let tmp = self.temp_path(&asset.uuid, r.kind)?;
            let hash = src.write(local_id, r.index, &tmp)?;
            self.place(asset, r.kind, &r.filename, &tmp, &hash)
                .with_context(|| format!("{} {}", asset.uuid, r.kind.name()))?;
        }
        self.sidecar(asset)
    }

    pub(crate) fn finish(self) -> Summary {
        let _ = std::fs::remove_dir(self.cfg.export_dir.join(".tmp"));
        self.summary
    }

    fn export(
        &mut self,
        src: &dyn Source,
        n: usize,
        total: usize,
        uuid: &str,
        local_id: &str,
        asset: Option<&Asset>,
    ) {
        self.pos = (n, total);
        let Some(asset) = asset else {
            self.fail(uuid, "no metadata, skipping");
            return;
        };
        if !self.cfg.filter.matches(asset) {
            self.summary.filtered += 1;
            return;
        }
        self.emit(Event::Asset {
            n,
            total,
            name: asset.filename.clone(),
            source_id: Some(local_id.to_owned()),
        });
        if let Err(e) = self.asset(src, asset, local_id) {
            self.fail(&asset.filename, format!("{e:#}"));
        }
    }
}

/// The asset as last recorded for its export: the sidecar if there is one,
/// otherwise the packet embedded in one of its files.
pub(crate) fn stored_asset(
    state: &State,
    dir: &str,
    stem: &str,
    uuid: &str,
) -> Result<Option<Asset>> {
    if let Ok(text) = std::fs::read_to_string(Path::new(dir).join(format!("{stem}.xmp"))) {
        return Ok(crate::photo::xmp::read(&text));
    }
    for (kind, path) in state.exports_of(uuid)? {
        if kind == Kind::Adjustments {
            continue;
        }
        if let Ok(Some(packet)) = crate::photo::embed::read(Path::new(&path))
            && let Some(asset) = crate::photo::xmp::read(&String::from_utf8_lossy(&packet))
        {
            return Ok(Some(asset));
        }
    }
    Ok(None)
}

/// Writes the packet into every media file of the asset. Files shared with
/// another asset through content dedup are left alone, since they can only
/// carry one asset's metadata.
pub(crate) fn embed_asset(
    state: &State,
    export_dir: &Path,
    uuid: &str,
    xmp: &str,
    (n, total): (usize, usize),
    progress: Progress,
) -> Result<()> {
    let rel = |p: &str| {
        Path::new(p)
            .strip_prefix(export_dir)
            .map(|r| r.display().to_string())
            .unwrap_or_else(|_| p.to_owned())
    };
    for (kind, path) in state.exports_of(uuid)? {
        if kind == Kind::Adjustments || state.shared(&path)? {
            continue;
        }
        match crate::photo::embed::embed(Path::new(&path), xmp) {
            Ok(Embed::Written) => progress(Event::Embedded {
                n,
                total,
                path: rel(&path),
            }),
            Ok(Embed::Unchanged | Embed::Unsupported) => {}
            Err(e) => progress(Event::error_at(
                n,
                total,
                rel(&path),
                format!("embed: {e:#}"),
            )),
        }
    }
    Ok(())
}

pub(crate) fn run(
    src: &dyn Source,
    cfg: &Config,
    state: &State,
    assets: &HashMap<String, Asset>,
    ids: &[(String, String)],
    progress: Progress,
    dry: bool,
) -> Summary {
    let (tx, rx) = mpsc::channel();
    for item in ids.iter().enumerate() {
        tx.send(item).unwrap();
    }
    drop(tx);
    let rx = Mutex::new(rx);
    let mut all = Exporter::new(cfg, state, progress, dry);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..cfg.concurrency)
            .map(|_| {
                scope.spawn(|| {
                    let mut ex = Exporter::new(cfg, state, progress, dry);
                    loop {
                        let next = rx.lock().unwrap().recv();
                        let Ok((n, (uuid, local_id))) = next else {
                            return ex.summary;
                        };
                        ex.export(src, n + 1, ids.len(), uuid, local_id, assets.get(uuid));
                    }
                })
            })
            .collect();
        for w in workers {
            all.summary.add(w.join().unwrap());
        }
    });
    all.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Resource;

    #[test]
    fn writes_what_it_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let q = dir.path().join("g");
        write_hashed(b"hello".as_slice(), &q).unwrap();
        assert_eq!(std::fs::read(q).unwrap(), b"hello");
    }

    #[test]
    fn adopts_identical_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let (tmp, dest) = (dir.path().join("t"), dir.path().join("IMG_1.jpg"));
        std::fs::write(&dest, b"same").unwrap();
        std::fs::write(&tmp, b"same").unwrap();
        let hash = blake3::hash(b"same").to_hex().to_string();
        assert_eq!(
            link_unique(&tmp, &dest, "uuid1234", &hash).unwrap(),
            Placed::Duplicate(dest.to_string_lossy().into_owned())
        );
        assert!(!tmp.exists());
        std::fs::write(&tmp, b"other").unwrap();
        let other = blake3::hash(b"other").to_hex().to_string();
        assert_eq!(
            link_unique(&tmp, &dest, "uuid1234", &other).unwrap(),
            Placed::New(
                dir.path()
                    .join("IMG_1_uuid1234.jpg")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }

    type Blob = (Kind, &'static str, &'static [u8]);
    struct Fake(HashMap<&'static str, Vec<Blob>>);

    impl Source for Fake {
        fn resources(&self, _asset: &Asset, id: &str) -> Result<Vec<Resource>> {
            Ok(self.0[id]
                .iter()
                .enumerate()
                .map(|(index, (kind, filename, _))| Resource {
                    index,
                    kind: *kind,
                    filename: filename.to_string(),
                })
                .collect())
        }
        fn write(&self, id: &str, index: usize, dest: &Path) -> Result<String> {
            write_hashed(self.0[id][index].2, dest)
        }
    }

    fn asset(uuid: &str, filename: &str, keyword: &str) -> Asset {
        Asset {
            uuid: uuid.into(),
            filename: filename.into(),
            keywords: vec![keyword.into()],
            taken: chrono::DateTime::parse_from_rfc3339("2001-01-01T00:00:00Z").ok(),
            ..Default::default()
        }
    }

    /// Little-endian TIFF whose IFD0 holds only ModifyDate.
    fn tiff_dated(date: &str) -> &'static [u8] {
        let mut d = b"II*\0\x08\0\0\0".to_vec();
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&0x0132u16.to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&20u32.to_le_bytes());
        d.extend_from_slice(&26u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(date.as_bytes());
        d.push(0);
        Box::leak(d.into_boxed_slice())
    }

    #[test]
    fn dates_undated_assets_from_the_file_before_the_upload_time() {
        let out = tempfile::tempdir().unwrap();
        let cfg = Config::parse(&format!(
            "export_dir = {}\npath = \"{{take.year}}\"\nembed = false",
            toml::Value::String(out.path().to_string_lossy().into_owned())
        ))
        .unwrap();
        let state = State::open(&out.path().join("state.sqlite")).unwrap();
        let uploaded = chrono::DateTime::parse_from_rfc3339("2016-11-28T10:00:00Z").unwrap();
        let undated = |uuid: &str, name: &str| Asset {
            uuid: uuid.into(),
            filename: name.into(),
            uploaded: Some(uploaded),
            ..Default::default()
        };
        let assets = HashMap::from([
            ("F".to_string(), undated("F", "f1902208.tif")),
            ("N".to_string(), undated("N", "nodate.bin")),
        ]);
        let src = Fake(HashMap::from([
            (
                "F/L0",
                vec![(
                    Kind::Original,
                    "f1902208.tif",
                    tiff_dated("2008:09:22 18:48:43"),
                )],
            ),
            (
                "N/L0",
                vec![(Kind::Original, "nodate.bin", b"no metadata here".as_slice())],
            ),
        ]));
        let ids = vec![
            ("F".to_string(), "F/L0".to_string()),
            ("N".to_string(), "N/L0".to_string()),
        ];
        let s = run(
            &src,
            &cfg,
            &state,
            &assets,
            &ids,
            &|e| eprintln!("{e}"),
            false,
        );
        assert_eq!((s.exported, s.errors), (2, 0));
        assert!(
            out.path().join("2008/f1902208.tif").exists(),
            "dated from the file"
        );
        assert!(
            out.path().join("2016/nodate.bin").exists(),
            "fell back to the upload time"
        );
        let stored = state.get("taken:F").unwrap().unwrap();
        assert!(stored.starts_with("2008-09-22T18:48:43"));
        assert_eq!(state.get("taken:N").unwrap(), None);
        let xmp = std::fs::read_to_string(out.path().join("2008/f1902208.xmp")).unwrap();
        assert!(xmp.contains("<exif:DateTimeOriginal>2008-09-22T18:48:43"));

        state.set("taken:N", "2009-01-01T00:00:00+00:00").unwrap();
        let s = run(
            &src,
            &cfg,
            &state,
            &assets,
            &ids,
            &|e| eprintln!("{e}"),
            false,
        );
        assert_eq!(s.skipped, 2);
        let xmp = std::fs::read_to_string(out.path().join("2016/nodate.xmp")).unwrap();
        assert!(
            xmp.contains("<exif:DateTimeOriginal>2009-01-01"),
            "stored override wins on rerun"
        );
    }

    #[test]
    fn without_sidecar_the_asset_is_read_back_from_the_file() {
        let out = tempfile::tempdir().unwrap();
        let cfg = Config::parse(&format!(
            "export_dir = {}\npath = \"{{take.year}}\"\nsidecar = false",
            toml::Value::String(out.path().to_string_lossy().into_owned())
        ))
        .unwrap();
        let state = State::open(&out.path().join("state.sqlite")).unwrap();
        let mut a = asset("F", "f.tif", "beach");
        a.title = Some("kept in the file".into());
        let assets = HashMap::from([("F".to_string(), a)]);
        let src = Fake(HashMap::from([(
            "F/L0",
            vec![(Kind::Original, "f.tif", tiff_dated("2008:09:22 18:48:43"))],
        )]));
        let ids = vec![("F".to_string(), "F/L0".to_string())];
        let s = run(
            &src,
            &cfg,
            &state,
            &assets,
            &ids,
            &|e| eprintln!("{e}"),
            false,
        );
        assert_eq!((s.exported, s.errors), (1, 0));
        let dir = out.path().join("2001");
        assert!(!dir.join("f.xmp").exists());
        let back = stored_asset(&state, dir.to_str().unwrap(), "f", "F")
            .unwrap()
            .unwrap();
        assert_eq!(back.title.as_deref(), Some("kept in the file"));
        assert_eq!(back.uuid, "F");
    }

    /// Exports A and B (same filename, keyword beach) and C (keyword city)
    /// under a `{take.date}_{keyword:0}_{stem}` filename, filtered to beach.
    fn export_templated(out: &Path) -> (State, Summary) {
        let cfg = Config::parse(&format!(
            "export_dir = {}\npath = \"{{take.year}}\"\nfilename = \"{{take.date}}_{{keyword:0}}_{{stem}}\"\nembed = false\nconcurrency = 1\n[filter]\nkeywords = [\"beach\"]",
            toml::Value::String(out.to_string_lossy().into_owned())
        ))
        .unwrap();
        let state = State::open(&out.join("state.sqlite")).unwrap();
        let assets = HashMap::from([
            ("A".to_string(), asset("A", "IMG_1.HEIC", "beach")),
            ("B".to_string(), asset("B", "IMG_1.HEIC", "beach")),
            ("C".to_string(), asset("C", "IMG_2.JPG", "city")),
        ]);
        let src = Fake(HashMap::from([
            (
                "A/L0",
                vec![(Kind::Original, "IMG_1.HEIC", b"one".as_slice())],
            ),
            (
                "B/L0",
                vec![(Kind::Original, "IMG_1.HEIC", b"two".as_slice())],
            ),
            (
                "C/L0",
                vec![(Kind::Original, "IMG_2.JPG", b"three".as_slice())],
            ),
        ]));
        let ids: Vec<_> = ["A", "B", "C"]
            .iter()
            .map(|u| (u.to_string(), format!("{u}/L0")))
            .collect();
        let s = run(
            &src,
            &cfg,
            &state,
            &assets,
            &ids,
            &|e| eprintln!("{e}"),
            false,
        );
        (state, s)
    }

    #[test]
    fn names_by_filename_template_and_filters() {
        let out = tempfile::tempdir().unwrap();
        let (_, s) = export_templated(out.path());
        assert_eq!((s.exported, s.filtered, s.errors), (2, 1, 0));
        let mut names: Vec<_> = std::fs::read_dir(out.path().join("2001"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "2001-01-01_beach_IMG_1.HEIC",
                "2001-01-01_beach_IMG_1.xmp",
                "2001-01-01_beach_IMG_1_B.HEIC",
                "2001-01-01_beach_IMG_1_B.xmp",
            ]
        );
        assert!(!out.path().join("2001").join("IMG_2.JPG").exists());
    }

    #[test]
    fn consolidate_renames_after_filename_template_change() {
        let out = tempfile::tempdir().unwrap();
        let (state, _) = export_templated(out.path());
        let cfg = Config::parse(&format!(
            "export_dir = {}\npath = \"{{take.year}}\"\nfilename = \"{{stem}}\"\nembed = false",
            toml::Value::String(out.path().to_string_lossy().into_owned())
        ))
        .unwrap();
        let c = crate::consolidate::run(&cfg, &state, &|e| eprintln!("{e}"), false).unwrap();
        assert_eq!((c.moved, c.restemmed), (2, 2));
        assert!(out.path().join("2001/IMG_1.HEIC").exists());
        assert!(out.path().join("2001/IMG_1_B.HEIC").exists());
        assert_eq!(state.stem("B").unwrap().as_deref(), Some("IMG_1_B"));
    }

    #[test]
    fn dry_run_plans_without_touching_anything() {
        let out = tempfile::tempdir().unwrap();
        let cfg = Config::parse(&format!(
            "export_dir = {}\npath = \"{{take.year}}/{{keyword:0}}\"",
            toml::Value::String(out.path().to_string_lossy().into_owned())
        ))
        .unwrap();
        let state = State::open(&out.path().join("state.sqlite")).unwrap();
        let assets = HashMap::from([("A".to_string(), asset("A", "IMG_1.HEIC", "beach"))]);
        let src = Fake(HashMap::from([(
            "A/L0",
            vec![(Kind::Original, "IMG_1.HEIC", b"same".as_slice())],
        )]));
        let ids = vec![("A".to_string(), "A/L0".to_string())];
        let mut log: Vec<Event> = Vec::new();
        let events = Mutex::new(&mut log);
        let s = run(
            &src,
            &cfg,
            &state,
            &assets,
            &ids,
            &|e| events.lock().unwrap().push(e),
            true,
        );
        assert_eq!((s.exported, s.skipped, s.errors), (1, 0, 0));
        assert!(
            log.iter().any(
                |e| matches!(e, Event::Placed { dest, .. } if Path::new(dest) == Path::new("2001/beach/IMG_1.HEIC"))
            ),
            "{log:?}"
        );
        assert!(!out.path().join("2001").exists(), "no folders created");
        assert_eq!(state.stem("A").unwrap(), None, "nothing claimed");
        assert_eq!(state.exported("A", Kind::Original).unwrap(), None);
        let s = run(
            &src,
            &cfg,
            &state,
            &assets,
            &ids,
            &|e| eprintln!("{e}"),
            false,
        );
        assert_eq!(s.exported, 1, "the real run still exports it");
    }

    #[test]
    fn exports_dedups_and_skips_on_rerun() {
        let out = tempfile::tempdir().unwrap();
        let cfg = Config::parse(&format!(
            "export_dir = {}\npath = \"{{take.year}}/{{keyword:0}}\"\nconcurrency = 1\nedits = true",
            toml::Value::String(out.path().to_string_lossy().into_owned())
        ))
        .unwrap();
        let state = State::open(&out.path().join("state.sqlite")).unwrap();
        let assets = HashMap::from([
            ("A".to_string(), asset("A", "IMG_1.HEIC", "beach")),
            ("B".to_string(), asset("B", "IMG_1.HEIC", "beach")),
            ("C".to_string(), asset("C", "IMG_2.JPG", "city")),
        ]);
        let src = Fake(HashMap::from([
            (
                "A/L0",
                vec![
                    (Kind::Original, "IMG_1.HEIC", b"same".as_slice()),
                    (Kind::Edited, "FullSizeRender.jpeg", b"edit"),
                    (Kind::Adjustments, "x.plist", b"aae"),
                ],
            ),
            ("B/L0", vec![(Kind::Original, "IMG_1.HEIC", b"same")]),
            ("C/L0", vec![(Kind::Original, "IMG_2.JPG", b"other")]),
        ]));
        let ids = vec![
            ("A".to_string(), "A/L0".to_string()),
            ("B".to_string(), "B/L0".to_string()),
            ("C".to_string(), "C/L0".to_string()),
            ("Z".to_string(), "Z/L0".to_string()),
        ];

        let s = run(
            &src,
            &cfg,
            &state,
            &assets,
            &ids,
            &|e| eprintln!("{e}"),
            false,
        );
        assert_eq!(
            (s.exported, s.duplicates, s.skipped, s.errors),
            (4, 1, 0, 1)
        );
        let beach = out.path().join("2001/beach");
        assert_eq!(std::fs::read(beach.join("IMG_1.HEIC")).unwrap(), b"same");
        assert_eq!(
            std::fs::read(beach.join("IMG_1_edited.jpeg")).unwrap(),
            b"edit"
        );
        assert_eq!(std::fs::read(beach.join("IMG_1.AAE")).unwrap(), b"aae");
        assert_eq!(
            std::fs::read(out.path().join("2001/city/IMG_2.JPG")).unwrap(),
            b"other"
        );
        let mut names: Vec<_> = std::fs::read_dir(&beach)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert!(!out.path().join(".tmp").exists());
        assert_eq!(
            names,
            ["IMG_1.AAE", "IMG_1.HEIC", "IMG_1.xmp", "IMG_1_edited.jpeg"]
        );
        assert!(state.exported("B", Kind::Original).unwrap().is_some());
        assert_eq!(state.stem("B").unwrap(), None);
        let sidecar =
            crate::photo::xmp::read(&std::fs::read_to_string(beach.join("IMG_1.xmp")).unwrap())
                .unwrap();
        assert_eq!(sidecar.keywords, ["beach"]);

        let s = run(
            &src,
            &cfg,
            &state,
            &assets,
            &ids,
            &|e| eprintln!("{e}"),
            false,
        );
        assert_eq!(
            (s.exported, s.duplicates, s.skipped, s.errors),
            (0, 0, 5, 1)
        );
    }
}
