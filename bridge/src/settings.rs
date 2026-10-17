use photosink::config::{self, Config};
use photosink::photo::{self, DateZone, Filter};
use photosink::{App, template};
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct Summaries {
    pub layout: String,
    pub dates: String,
    pub filter: String,
    pub files: String,
    pub filter_hint: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DateSourceOption {
    pub value: DateSource,
    pub label: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ZoneOption {
    pub value: ZoneMode,
    pub label: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MetadataOption {
    pub value: Metadata,
    pub label: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MediaOption {
    pub value: Media,
    pub label: String,
}

/// Choices and copy for the settings panel, so every platform says the same thing.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Options {
    pub date_sources: Vec<DateSourceOption>,
    pub date_source_hint: String,
    pub zones: Vec<ZoneOption>,
    pub zone_hint: String,
    pub offset_placeholder: String,
    pub media: Vec<MediaOption>,
    pub favorites_label: String,
    pub list_placeholder: String,
    pub edits_off: String,
    pub edits_on: String,
    pub edits_hint: String,
    pub metadata: Vec<MetadataOption>,
    pub metadata_hint: String,
    pub folder_placeholder: String,
    pub filename_placeholder: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Field {
    pub name: String,
    pub help: String,
    /// What inserting the field adds, `{album:0}` for indexed fields.
    pub token: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FieldGroup {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Preview {
    /// The rendered path under the export folder's own name, `Photos/2019/05/IMG_0421.HEIC`.
    pub line: String,
    pub root: String,
    pub dir: String,
    pub file: String,
    pub ext: String,
    /// Set when the template being edited can't be saved.
    pub problem: Option<String>,
    pub sample_note: String,
    /// Field names the template being edited uses.
    pub used: Vec<String>,
}

/// A template after inserting a field; `caret` counts UTF-16 code units.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Insertion {
    pub text: String,
    pub caret: u32,
}

pub fn options() -> Options {
    let here = if cfg!(target_os = "macos") {
        "This Mac's zone"
    } else {
        "This computer's zone"
    };
    Options {
        date_sources: vec![
            DateSourceOption {
                value: DateSource::Source,
                label: "Source metadata first".into(),
            },
            DateSourceOption {
                value: DateSource::File,
                label: "File EXIF first".into(),
            },
        ],
        date_source_hint: "falls back to the other, then the upload time".into(),
        zones: vec![
            ZoneOption {
                value: ZoneMode::Asset,
                label: "As recorded".into(),
            },
            ZoneOption {
                value: ZoneMode::Local,
                label: here.into(),
            },
            ZoneOption {
                value: ZoneMode::Utc,
                label: "UTC".into(),
            },
            ZoneOption {
                value: ZoneMode::Fixed,
                label: "Fixed offset".into(),
            },
        ],
        zone_hint: "the zone the folder date is read in".into(),
        offset_placeholder: "+02:00".into(),
        media: vec![
            MediaOption {
                value: Media::All,
                label: "All".into(),
            },
            MediaOption {
                value: Media::Photos,
                label: "Photos".into(),
            },
            MediaOption {
                value: Media::Videos,
                label: "Videos".into(),
            },
        ],
        favorites_label: "Favourites only".into(),
        list_placeholder: "Comma separated; any match".into(),
        edits_off: "Originals".into(),
        edits_on: "Originals + edits".into(),
        edits_hint: "edits are the rendered version plus the .AAE adjustment data".into(),
        metadata: vec![
            MetadataOption {
                value: Metadata::Both,
                label: "In files + sidecar".into(),
            },
            MetadataOption {
                value: Metadata::Embed,
                label: "In files".into(),
            },
            MetadataOption {
                value: Metadata::Sidecar,
                label: "Sidecar only".into(),
            },
        ],
        metadata_hint: "the XMP packet written into each file, to a .xmp beside it, or both".into(),
        folder_placeholder: "{take.year}/{take.month}".into(),
        filename_placeholder: "{stem}".into(),
    }
}

pub fn fields() -> Vec<FieldGroup> {
    let mut out: Vec<FieldGroup> = Vec::new();
    for f in photo::FIELDS {
        let field = Field {
            name: f.name.into(),
            help: f.help.into(),
            token: if f.indexed {
                format!("{{{}:0}}", f.name)
            } else {
                format!("{{{}}}", f.name)
            },
        };
        match out.iter_mut().find(|g| g.name == f.group) {
            Some(g) => g.fields.push(field),
            None => out.push(FieldGroup {
                name: f.group.into(),
                fields: vec![field],
            }),
        }
    }
    out
}

fn utf16_to_byte(s: &str, units: u32) -> usize {
    let mut seen = 0u32;
    for (i, c) in s.char_indices() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16() as u32;
    }
    s.len()
}

/// Replaces the selection `start..end` (UTF-16 offsets) with `token`, adding
/// a separator when the text before doesn't already end in one.
pub fn insert_field(
    text: &str,
    start: u32,
    end: u32,
    token: &str,
    kind: TemplateKind,
) -> Insertion {
    let (start, end) = (start.min(end), start.max(end));
    let (a, b) = (utf16_to_byte(text, start), utf16_to_byte(text, end));
    let before = &text[..a];
    let joiner = match kind {
        TemplateKind::Folder => "/",
        TemplateKind::Filename => "_",
    };
    let sep = match before.chars().last() {
        None => "",
        Some(c) if "/_-{|.".contains(c) => "",
        Some(_) => joiner,
    };
    let head = format!("{before}{sep}{token}");
    Insertion {
        caret: head.encode_utf16().count() as u32,
        text: format!("{head}{}", &text[b..]),
    }
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

fn date(s: &str, what: &str) -> anyhow::Result<Option<chrono::NaiveDate>> {
    match s.trim() {
        "" => Ok(None),
        d => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| anyhow::anyhow!("{what} date {d:?} is not YYYY-MM-DD")),
    }
}

impl Settings {
    pub fn from_config(cfg: &Config) -> Settings {
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

    pub fn zone_value(&self) -> String {
        match self.zone {
            ZoneMode::Asset => "asset".into(),
            ZoneMode::Local => "local".into(),
            ZoneMode::Utc => "utc".into(),
            ZoneMode::Fixed if self.offset.trim().is_empty() => "+00:00".into(),
            ZoneMode::Fixed => self.offset.trim().into(),
        }
    }

    pub fn filter(&self) -> anyhow::Result<Filter> {
        Ok(Filter {
            media: match self.media {
                Media::All => photo::Media::All,
                Media::Photos => photo::Media::Photos,
                Media::Videos => photo::Media::Videos,
            },
            favorites: self.favorites,
            from: date(&self.from, "from")?,
            to: date(&self.to, "to")?,
            albums: split_list(&self.albums),
            keywords: split_list(&self.keywords),
        })
    }

    /// Writes the settings into the config file, keeping keys the panel
    /// doesn't show. Nothing is written if the result doesn't parse.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let filter = self.filter()?;
        let zone = self.zone_value();
        zone.parse::<DateZone>().map_err(anyhow::Error::msg)?;
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
            if let Ok(filter) = toml::Value::try_from(&filter) {
                t.insert("filter".into(), filter);
            }
        })
    }

    pub fn summaries(&self) -> Summaries {
        let zone = options()
            .zones
            .into_iter()
            .find(|z| z.value == self.zone && z.value != ZoneMode::Fixed)
            .map(|z| {
                z.label
                    .to_lowercase()
                    .replace("utc", "UTC")
                    .replace("this mac", "this Mac")
            })
            .unwrap_or_else(|| format!("UTC{}", self.zone_value()));
        let mut filter = Vec::new();
        match self.media {
            Media::All => {}
            Media::Photos => filter.push("photos".to_owned()),
            Media::Videos => filter.push("videos".to_owned()),
        }
        if self.favorites {
            filter.push("favourites".into());
        }
        if !self.from.trim().is_empty() || !self.to.trim().is_empty() {
            let end = |s: &str| {
                if s.trim().is_empty() {
                    "…".to_owned()
                } else {
                    s.trim().to_owned()
                }
            };
            filter.push(format!("{} to {}", end(&self.from), end(&self.to)));
        }
        let albums = split_list(&self.albums);
        if !albums.is_empty() {
            filter.push(format!("albums: {}", albums.join(", ")));
        }
        let keywords = split_list(&self.keywords);
        if !keywords.is_empty() {
            filter.push(format!("keywords: {}", keywords.join(", ")));
        }
        let filtering = !filter.is_empty();
        Summaries {
            layout: format!(
                "{}/{}",
                if self.path.is_empty() {
                    "…"
                } else {
                    &self.path
                },
                if self.filename.is_empty() {
                    "{stem}"
                } else {
                    &self.filename
                }
            ),
            dates: format!(
                "{} · {zone}",
                match self.date_source {
                    DateSource::Source => "source metadata first",
                    DateSource::File => "file EXIF first",
                }
            ),
            filter: if filtering {
                filter.join(" · ")
            } else {
                "everything".into()
            },
            files: format!(
                "{} · metadata {}",
                if self.edits {
                    "originals + edits"
                } else {
                    "originals"
                },
                match self.metadata {
                    Metadata::Both => "in files + sidecar",
                    Metadata::Embed => "in files",
                    Metadata::Sidecar => "in sidecar",
                }
            ),
            filter_hint: if filtering {
                "Only matching assets are exported. Consolidate and Dates ignore the filter."
            } else {
                "Everything is exported."
            }
            .into(),
        }
    }
}

fn leaf(dir: &str) -> String {
    Path::new(dir.trim_end_matches('/'))
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Export".into())
}

/// Renders both templates against the first exported asset, or a sample,
/// and reports what stops the `kind` template being saved.
pub fn preview(export_dir: &str, path: &str, filename: &str, kind: TemplateKind) -> Preview {
    let p = match App::open(None, None) {
        Ok(app) => app.preview_path(path, filename).ok(),
        Err(_) => None,
    }
    .unwrap_or_else(|| {
        photosink::preview_path(None, path, filename, &photo::Asset::sample(), "sample")
    });
    let (tpl, unknown) = match kind {
        TemplateKind::Folder => (path, &p.unknown_path),
        TemplateKind::Filename => (filename, &p.unknown_filename),
    };
    let root = leaf(export_dir);
    let dir = if p.dir.is_empty() {
        "…".to_owned()
    } else {
        p.dir.clone()
    };
    Preview {
        line: format!("{root}/{dir}/{}.{}", p.file, p.ext),
        root,
        dir,
        file: p.file,
        ext: p.ext,
        problem: (!unknown.is_empty()).then(|| {
            format!(
                "Unknown field{}: {}. Pick from the list; it won't be saved until it's fixed.",
                if unknown.len() == 1 { "" } else { "s" },
                unknown.join(", ")
            )
        }),
        sample_note: if p.sample == "sample" {
            "sample photo until something is exported".into()
        } else {
            format!("from {}", p.sample)
        },
        used: template::fields(tpl)
            .into_iter()
            .map(|(name, _)| name)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_with_separator() {
        let i = insert_field("{take.year}", 11, 11, "{take.month}", TemplateKind::Folder);
        assert_eq!(
            i,
            Insertion {
                text: "{take.year}/{take.month}".into(),
                caret: 24
            }
        );
        let i = insert_field("x-", 2, 2, "{stem}", TemplateKind::Filename);
        assert_eq!(i.text, "x-{stem}");
        let i = insert_field("ab", 0, 2, "{stem}", TemplateKind::Filename);
        assert_eq!(
            i,
            Insertion {
                text: "{stem}".into(),
                caret: 6
            }
        );
        let i = insert_field("é", 1, 1, "{stem}", TemplateKind::Filename);
        assert_eq!(i.text, "é_{stem}");
    }

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

    #[test]
    fn summarises() {
        let s = Settings {
            export_dir: "/x".into(),
            path: "{take.year}".into(),
            filename: String::new(),
            date_source: DateSource::File,
            zone: ZoneMode::Fixed,
            offset: "+02:00".into(),
            edits: true,
            metadata: Metadata::Sidecar,
            media: Media::Videos,
            favorites: true,
            from: "2019-01-01".into(),
            to: String::new(),
            albums: "Trip".into(),
            keywords: String::new(),
        };
        let m = s.summaries();
        assert_eq!(m.layout, "{take.year}/{stem}");
        assert_eq!(m.dates, "file EXIF first · UTC+02:00");
        assert_eq!(
            m.filter,
            "videos · favourites · 2019-01-01 to … · albums: Trip"
        );
        assert_eq!(m.files, "originals + edits · metadata in sidecar");
    }
}
