use crate::photo::{Asset, DateZone, Filter};
use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub export_dir: PathBuf,
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default = "default_filename")]
    pub filename: String,
    #[serde(default)]
    pub date_source: DateSource,
    #[serde(default)]
    pub date_zone: DateZone,
    #[serde(default)]
    pub filter: Filter,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default)]
    pub edits: bool,
    #[serde(default = "default_true")]
    pub embed: bool,
    #[serde(default = "default_true")]
    pub sidecar: bool,
    #[serde(default = "default_client_id")]
    pub onedrive_client_id: String,
    #[serde(default)]
    pub dropbox_app_key: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_concurrency() -> usize {
    4
}

fn default_client_id() -> String {
    "cf81c004-3e08-4c30-804c-92271c32669d".into()
}

fn default_path() -> String {
    "{take.year}/{take.month}".into()
}

fn default_filename() -> String {
    "{stem}".into()
}

/// Where an asset's taken date comes from first.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DateSource {
    /// The source's metadata, then the file, then the upload time.
    #[default]
    Source,
    /// The file's own EXIF or container date, then the source, then the upload time.
    File,
}

impl Config {
    /// The asset as the path templates see it.
    pub fn zoned(&self, asset: &Asset) -> Asset {
        asset.zoned(self.date_zone)
    }
}

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "photosink").context("no home directory")
}

pub fn expand_home(p: &Path) -> PathBuf {
    match (p.strip_prefix("~"), BaseDirs::new()) {
        (Ok(rest), Some(base)) => base.home_dir().join(rest),
        _ => p.to_path_buf(),
    }
}

pub fn write_initial(path: &Path) -> Result<()> {
    anyhow::ensure!(!path.exists(), "{} already exists", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, include_str!("../config.example.toml"))?;
    Ok(())
}

pub fn update(path: &Path, set: impl FnOnce(&mut toml::Table)) -> Result<()> {
    if !path.exists() {
        write_initial(path)?;
    }
    let mut table: toml::Table = std::fs::read_to_string(path)?.parse()?;
    set(&mut table);
    let text = toml::to_string(&table)?;
    Config::parse(&text)?;
    std::fs::write(path, text)?;
    Ok(())
}

impl Config {
    pub fn parse(s: &str) -> Result<Config> {
        let mut cfg: Config = toml::from_str(s)?;
        cfg.export_dir = expand_home(&cfg.export_dir);
        anyhow::ensure!(cfg.concurrency > 0, "concurrency must be > 0");
        for (what, tpl) in [("path", &cfg.path), ("filename", &cfg.filename)] {
            let unknown = crate::template::unknown(tpl, &crate::photo::field_names());
            anyhow::ensure!(
                unknown.is_empty(),
                "{what} uses unknown field{} {}",
                if unknown.len() == 1 { "" } else { "s" },
                unknown.join(", ")
            );
        }
        anyhow::ensure!(
            cfg.filter
                .from
                .zip(cfg.filter.to)
                .is_none_or(|(f, t)| f <= t),
            "filter.from is after filter.to"
        );
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Config> {
        let s =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Config::parse(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_path_fields() {
        let err = Config::parse("export_dir = \"/o\"\npath = \"{take.yaer}/{keyword:0}\"")
            .unwrap_err()
            .to_string();
        assert!(err.contains("take.yaer"), "{err}");
        assert!(
            Config::parse("export_dir = \"/o\"\npath = \"{take.year}/{keyword:0|label:0}\"")
                .is_ok()
        );
    }

    #[test]
    fn parses_dates_and_filters() {
        let cfg = Config::parse(
            "export_dir = \"/o\"\nfilename = \"{take.date}_{stem}\"\ndate_source = \"file\"\ndate_zone = \"+02:00\"\n[filter]\nmedia = \"photos\"\nfavorites = true\nfrom = \"2019-01-01\"\nalbums = [\"Trip\"]",
        )
        .unwrap();
        assert_eq!(cfg.date_source, DateSource::File);
        assert_eq!(cfg.date_zone.to_string(), "+02:00");
        assert_eq!(cfg.filter.media, crate::photo::Media::Photos);
        assert!(cfg.filter.favorites);
        assert_eq!(cfg.filter.from.unwrap().to_string(), "2019-01-01");
        assert!(Config::parse("export_dir = \"/o\"\ndate_zone = \"mars\"").is_err());
        assert!(
            Config::parse(
                "export_dir = \"/o\"\n[filter]\nfrom = \"2020-01-01\"\nto = \"2019-01-01\""
            )
            .is_err()
        );
        assert!(Config::parse("export_dir = \"/o\"\nfilename = \"{nope}\"").is_err());
        assert_eq!(
            Config::parse("export_dir = \"/o\"").unwrap().filename,
            "{stem}"
        );
    }

    #[test]
    fn defaults() {
        let cfg = Config::parse("export_dir = \"/tmp/out\"").unwrap();
        assert_eq!(cfg.export_dir, PathBuf::from("/tmp/out"));
        assert_eq!(cfg.path, "{take.year}/{take.month}");
        assert_eq!(cfg.concurrency, 4);
        assert!(!cfg.edits);
    }

    #[test]
    fn expands_tilde() {
        let cfg = Config::parse("export_dir = \"~/x\"\npath = \"{uuid}\"").unwrap();
        assert!(!cfg.export_dir.starts_with("~"));
        assert!(cfg.export_dir.ends_with("x"));
    }

    #[test]
    fn writes_initial_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.toml");
        write_initial(&path).unwrap();
        assert!(
            Config::load(&path)
                .unwrap()
                .export_dir
                .ends_with("Pictures/Export")
        );
        assert!(write_initial(&path).is_err());
    }

    #[test]
    fn parses_sources() {
        let cfg = Config::parse("export_dir = \"/o\"\nonedrive_client_id = \"abc\"").unwrap();
        assert_eq!(cfg.onedrive_client_id, "abc");
        let cfg = Config::parse("export_dir = \"/o\"").unwrap();
        assert_eq!(cfg.onedrive_client_id, default_client_id());
    }

    #[test]
    fn updates_keys_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        update(&path, |t| {
            t.insert("export_dir".into(), "/out".into());
            t.insert("path".into(), "{uuid}".into());
        })
        .unwrap();
        update(&path, |t| {
            t.insert("export_dir".into(), "/other".into());
        })
        .unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.export_dir, PathBuf::from("/other"));
        assert_eq!(cfg.path, "{uuid}");
    }

    #[test]
    fn rejects_unknown_and_zero() {
        assert!(Config::parse("export_dir = \"/o\"\nbogus = 1").is_err());
        assert!(Config::parse("export_dir = \"/o\"\nconcurrency = 0").is_err());
    }
}
