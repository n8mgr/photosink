use photosink::config::{self, Config};
use photosink::photo::{self, Asset, DateZone, Filter};
use photosink::template;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DateSource {
    Source,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ZoneMode {
    Asset,
    Local,
    Utc,
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Metadata {
    Both,
    Embed,
    Sidecar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Media {
    All,
    Photos,
    Videos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TemplateKind {
    Folder,
    Filename,
}

/// Everything the settings panel edits. Dates are `YYYY-MM-DD` or empty;
/// albums and keywords are comma separated.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Settings {
    pub export_dir: String,
    pub path: String,
    pub filename: String,
    pub date_source: DateSource,
    pub zone: ZoneMode,
    pub offset: String,
    pub edits: bool,
    pub metadata: Metadata,
    pub media: Media,
    pub favorites: bool,
    pub from: String,
    pub to: String,
    pub albums: String,
    pub keywords: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FieldGroup {
    Date,
    Place,
    Tags,
    Camera,
    Other,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Field {
    pub name: String,
    pub group: FieldGroup,
    /// Takes an index, like `album`.
    pub indexed: bool,
    /// What inserting the field adds, `{album:0}` for indexed fields.
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TemplateCheck {
    /// Field names the template uses that don't exist; it can't be saved
    /// while any remain.
    pub unknown: Vec<String>,
    /// Field names the template uses.
    pub used: Vec<String>,
}

/// Both templates rendered for the preview `root/dir/file.ext`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Preview {
    /// The export folder's last path component, None when it has none.
    pub root: Option<String>,
    /// Empty when the folder template renders nothing.
    pub dir: String,
    pub file: String,
    pub ext: String,
    /// The stem of the exported asset previewed, None for the built-in sample.
    pub exported_stem: Option<String>,
    pub folder: TemplateCheck,
    pub filename: TemplateCheck,
}

/// Why settings can't be saved.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SettingsProblem {
    /// The from date isn't `YYYY-MM-DD`.
    FromDate {
        value: String,
    },
    /// The to date isn't `YYYY-MM-DD`.
    ToDate {
        value: String,
    },
    /// The fixed offset isn't `±HH:MM`.
    Offset {
        value: String,
    },
    UnknownFields {
        template: TemplateKind,
        names: Vec<String>,
    },
    FromAfterTo,
}

pub(crate) fn fields() -> Vec<Field> {
    photo::FIELDS
        .iter()
        .map(|f| Field {
            name: f.name.into(),
            group: match f.group {
                photo::FieldGroup::Date => FieldGroup::Date,
                photo::FieldGroup::Place => FieldGroup::Place,
                photo::FieldGroup::Tags => FieldGroup::Tags,
                photo::FieldGroup::Camera => FieldGroup::Camera,
                photo::FieldGroup::Other => FieldGroup::Other,
            },
            indexed: f.indexed,
            token: if f.indexed {
                format!("{{{}:0}}", f.name)
            } else {
                format!("{{{}}}", f.name)
            },
        })
        .collect()
}

fn zone_mode(zone: DateZone) -> (ZoneMode, String) {
    match zone {
        DateZone::Asset => (ZoneMode::Asset, String::new()),
        DateZone::Local => (ZoneMode::Local, String::new()),
        DateZone::Utc => (ZoneMode::Utc, String::new()),
        DateZone::Fixed(_) => (ZoneMode::Fixed, zone.to_string()),
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn date(
    s: &str,
    bad: fn(String) -> SettingsProblem,
) -> Result<Option<chrono::NaiveDate>, SettingsProblem> {
    match s.trim() {
        "" => Ok(None),
        d => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| bad(d.to_owned())),
    }
}

impl Settings {
    pub(crate) fn from_config(cfg: &Config) -> Settings {
        let (zone, offset) = zone_mode(cfg.date_zone);
        Settings {
            export_dir: cfg.export_dir.display().to_string(),
            path: cfg.path.clone(),
            filename: cfg.filename.clone(),
            date_source: match cfg.date_source {
                config::DateSource::Source => DateSource::Source,
                config::DateSource::File => DateSource::File,
            },
            zone,
            offset,
            edits: cfg.edits,
            metadata: match (cfg.embed, cfg.sidecar) {
                (true, false) => Metadata::Embed,
                (false, true) => Metadata::Sidecar,
                _ => Metadata::Both,
            },
            media: match cfg.filter.media {
                photo::Media::All => Media::All,
                photo::Media::Photos => Media::Photos,
                photo::Media::Videos => Media::Videos,
            },
            favorites: cfg.filter.favorites,
            from: cfg.filter.from.map(|d| d.to_string()).unwrap_or_default(),
            to: cfg.filter.to.map(|d| d.to_string()).unwrap_or_default(),
            albums: cfg.filter.albums.join(", "),
            keywords: cfg.filter.keywords.join(", "),
        }
    }

    pub(crate) fn zone_value(&self) -> String {
        match self.zone {
            ZoneMode::Asset => "asset".into(),
            ZoneMode::Local => "local".into(),
            ZoneMode::Utc => "utc".into(),
            ZoneMode::Fixed if self.offset.trim().is_empty() => "+00:00".into(),
            ZoneMode::Fixed => self.offset.trim().into(),
        }
    }

    fn filter(&self) -> Result<Filter, SettingsProblem> {
        Ok(Filter {
            media: match self.media {
                Media::All => photo::Media::All,
                Media::Photos => photo::Media::Photos,
                Media::Videos => photo::Media::Videos,
            },
            favorites: self.favorites,
            from: date(&self.from, |value| SettingsProblem::FromDate { value })?,
            to: date(&self.to, |value| SettingsProblem::ToDate { value })?,
            albums: split_list(&self.albums),
            keywords: split_list(&self.keywords),
        })
    }

    /// The first reason the settings can't be saved, if any.
    pub(crate) fn validate(&self) -> Result<Filter, SettingsProblem> {
        let filter = self.filter()?;
        if self.zone_value().parse::<DateZone>().is_err() {
            return Err(SettingsProblem::Offset {
                value: self.offset.trim().to_owned(),
            });
        }
        let names = photo::field_names();
        for (template, text) in [
            (TemplateKind::Folder, &self.path),
            (TemplateKind::Filename, &self.filename),
        ] {
            let unknown = template::unknown(text, &names);
            if !unknown.is_empty() {
                return Err(SettingsProblem::UnknownFields {
                    template,
                    names: unknown,
                });
            }
        }
        if filter.from.zip(filter.to).is_some_and(|(f, t)| f > t) {
            return Err(SettingsProblem::FromAfterTo);
        }
        Ok(filter)
    }

    /// Writes the settings into the config file, keeping keys the panel
    /// doesn't show. Nothing is written if they don't validate.
    pub(crate) fn save(&self, path: &Path) -> Result<(), crate::CoreError> {
        let filter = self
            .validate()
            .map_err(|problem| crate::CoreError::InvalidSettings { problem })?;
        let filter = toml::Value::try_from(filter).map_err(anyhow::Error::from)?;
        let zone = self.zone_value();
        config::update(path, |t| {
            t.insert("export_dir".into(), self.export_dir.clone().into());
            t.insert("path".into(), self.path.clone().into());
            t.insert("filename".into(), self.filename.clone().into());
            t.insert(
                "date_source".into(),
                match self.date_source {
                    DateSource::Source => "source",
                    DateSource::File => "file",
                }
                .into(),
            );
            t.insert("date_zone".into(), zone.into());
            t.insert("edits".into(), self.edits.into());
            t.insert("embed".into(), (self.metadata != Metadata::Sidecar).into());
            t.insert("sidecar".into(), (self.metadata != Metadata::Embed).into());
            t.insert("filter".into(), filter);
        })?;
        Ok(())
    }
}

fn leaf(dir: &str) -> Option<String> {
    Path::new(dir.trim_end_matches('/'))
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
}

fn check(template: &str, unknown: &[String]) -> TemplateCheck {
    TemplateCheck {
        unknown: unknown.to_vec(),
        used: template::fields(template)
            .into_iter()
            .map(|(name, _)| name)
            .collect(),
    }
}

/// Renders both templates of `s` against `exported`, an exported asset and
/// its stem, or the built-in sample without one.
pub(crate) fn preview(s: &Settings, exported: Option<&(Asset, String)>) -> Preview {
    let zone = s.zone_value().parse().unwrap_or_default();
    let sample = Asset::sample();
    let asset = exported.map_or(&sample, |(a, _)| a);
    let p = photosink::preview_path(zone, &s.path, &s.filename, asset);
    Preview {
        root: leaf(&s.export_dir),
        dir: p.dir,
        file: p.file,
        ext: p.ext,
        exported_stem: exported.map(|(_, stem)| stem.clone()),
        folder: check(&s.path, &p.unknown_path),
        filename: check(&s.filename, &p.unknown_filename),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "export_dir = \"/x\"\nconcurrency = 2\n").unwrap();
        let mut s = Settings::from_config(&Config::load(&path).unwrap());
        s.zone = ZoneMode::Fixed;
        s.offset = "+02:00".into();
        s.metadata = Metadata::Embed;
        s.albums = " Trip, , Beach ".into();
        s.from = "2019-01-01".into();
        s.save(&path).unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.concurrency, 2, "keys the panel doesn't show survive");
        assert!(cfg.embed && !cfg.sidecar);
        assert_eq!(cfg.filter.albums, vec!["Trip", "Beach"]);
        let back = Settings::from_config(&cfg);
        assert_eq!(back.zone, ZoneMode::Fixed);
        assert_eq!(back.offset, "+02:00");
        assert_eq!(back.albums, "Trip, Beach");
        s.from = "2019-13-01".into();
        assert!(s.save(&path).is_err());
        s.from = "2020-01-01".into();
        s.to = "2019-01-01".into();
        assert!(s.save(&path).is_err(), "from after to");
        s.to = String::new();
        s.path = "{take.yaer}".into();
        assert!(s.save(&path).is_err());
    }

    fn settings() -> Settings {
        Settings::from_config(&Config::parse("export_dir = \"/x/Out/\"").unwrap())
    }

    #[test]
    fn validates_with_typed_problems() {
        let mut s = settings();
        assert!(s.validate().is_ok());
        s.from = " 2019-13-01 ".into();
        assert_eq!(
            s.validate().unwrap_err(),
            SettingsProblem::FromDate {
                value: "2019-13-01".into()
            }
        );
        s.from = "2020-01-01".into();
        s.to = "2019-01-01".into();
        assert_eq!(s.validate().unwrap_err(), SettingsProblem::FromAfterTo);
        s.to = String::new();
        s.zone = ZoneMode::Fixed;
        s.offset = "mars".into();
        assert_eq!(
            s.validate().unwrap_err(),
            SettingsProblem::Offset {
                value: "mars".into()
            }
        );
        s.offset = "+02:00".into();
        s.filename = "{stem}_{nope}".into();
        assert_eq!(
            s.validate().unwrap_err(),
            SettingsProblem::UnknownFields {
                template: TemplateKind::Filename,
                names: vec!["nope".into()]
            }
        );
    }

    #[test]
    fn previews_typed_parts() {
        let mut s = settings();
        s.path = "{take.yaer}".into();
        let p = preview(&s, None);
        assert_eq!(p.root.as_deref(), Some("Out"));
        assert_eq!(p.exported_stem, None);
        assert_eq!(p.folder.unknown, vec!["take.yaer".to_owned()]);
        assert!(p.filename.unknown.is_empty());
        s.export_dir = String::new();
        assert_eq!(preview(&s, None).root, None);
    }

    #[test]
    fn fields_carry_tokens_and_groups() {
        let f = fields();
        let album = f.iter().find(|f| f.name == "album").unwrap();
        assert_eq!(album.token, "{album:0}");
        assert_eq!(album.group, FieldGroup::Tags);
        assert_eq!(f[0].token, "{take.year}");
    }
}
