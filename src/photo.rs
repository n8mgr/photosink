pub(crate) mod embed;
pub(crate) mod exif;
pub(crate) mod geo;
pub(crate) mod xmp;

pub use geo::Place;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Camera {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Asset {
    pub uuid: String,
    pub filename: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub taken: Option<DateTime<FixedOffset>>,
    pub uploaded: Option<DateTime<FixedOffset>>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub favorite: bool,
    pub video: bool,
    pub keywords: Vec<String>,
    pub albums: Vec<String>,
    pub labels: Vec<String>,
    pub people: Vec<String>,
    pub camera: Camera,
    pub place: Place,
}

/// Which clock the date fields of a path are read in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DateZone {
    /// The offset the date was recorded with.
    #[default]
    Asset,
    Local,
    Utc,
    Fixed(FixedOffset),
}

impl std::str::FromStr for DateZone {
    type Err = String;

    fn from_str(s: &str) -> Result<DateZone, String> {
        match s.trim() {
            "" | "asset" => Ok(DateZone::Asset),
            "local" => Ok(DateZone::Local),
            "utc" | "UTC" => Ok(DateZone::Utc),
            offset => {
                let fmt = |s: &str| {
                    DateTime::parse_from_rfc3339(&format!("2000-01-01T00:00:00{s}"))
                        .map(|t| *t.offset())
                };
                fmt(offset)
                    .map(DateZone::Fixed)
                    .map_err(|_| format!("date_zone {offset:?} is not asset, local, utc or ±HH:MM"))
            }
        }
    }
}

impl std::fmt::Display for DateZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DateZone::Asset => f.write_str("asset"),
            DateZone::Local => f.write_str("local"),
            DateZone::Utc => f.write_str("utc"),
            DateZone::Fixed(o) => write!(f, "{o}"),
        }
    }
}

impl Serialize for DateZone {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DateZone {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<DateZone, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Media {
    #[default]
    All,
    Photos,
    Videos,
}

/// Which assets a run exports. Everything is a conjunction; empty lists and
/// `None` dates do not constrain. An asset with no date at all passes a date
/// range, since nothing is known to exclude it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Filter {
    pub media: Media,
    pub favorites: bool,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub albums: Vec<String>,
    pub keywords: Vec<String>,
}

impl Filter {
    pub fn matches(&self, a: &Asset) -> bool {
        let media_ok = match self.media {
            Media::All => true,
            Media::Photos => !a.video,
            Media::Videos => a.video,
        };
        let date_ok = match a.taken.or(a.uploaded).map(|t| t.date_naive()) {
            Some(d) => self.from.is_none_or(|f| d >= f) && self.to.is_none_or(|t| d <= t),
            None => true,
        };
        let any = |wanted: &[String], have: &[String]| {
            wanted.is_empty()
                || have.iter().any(|h| {
                    wanted
                        .iter()
                        .any(|w| w.trim().eq_ignore_ascii_case(h.trim()))
                })
        };
        media_ok
            && date_ok
            && (!self.favorites || a.favorite)
            && any(&self.albums, &a.albums)
            && any(&self.keywords, &a.keywords)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FieldGroup {
    Date,
    Place,
    Tags,
    Camera,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct FieldInfo {
    pub name: &'static str,
    pub group: FieldGroup,
    /// Takes an index, `{album:0}`.
    pub indexed: bool,
}

const fn f(name: &'static str, group: FieldGroup, indexed: bool) -> FieldInfo {
    FieldInfo {
        name,
        group,
        indexed,
    }
}

/// Every name `Asset::field` understands, for the path editor.
pub const FIELDS: &[FieldInfo] = &[
    f("take.year", FieldGroup::Date, false),
    f("take.month", FieldGroup::Date, false),
    f("take.day", FieldGroup::Date, false),
    f("take.date", FieldGroup::Date, false),
    f("place.name", FieldGroup::Place, false),
    f("place.city", FieldGroup::Place, false),
    f("place.state", FieldGroup::Place, false),
    f("place.country", FieldGroup::Place, false),
    f("keyword", FieldGroup::Tags, true),
    f("album", FieldGroup::Tags, true),
    f("label", FieldGroup::Tags, true),
    f("people", FieldGroup::Tags, true),
    f("camera.make", FieldGroup::Camera, false),
    f("camera.model", FieldGroup::Camera, false),
    f("camera.lens", FieldGroup::Camera, false),
    f("title", FieldGroup::Other, false),
    f("type", FieldGroup::Other, false),
    f("stem", FieldGroup::Other, false),
    f("uuid", FieldGroup::Other, false),
];

pub fn field_names() -> Vec<&'static str> {
    FIELDS.iter().map(|f| f.name).collect()
}

impl Asset {
    /// A representative asset for previewing a path template.
    pub fn sample() -> Asset {
        Asset {
            uuid: "9F1C2A7E-SAMPLE".into(),
            filename: "IMG_0421.HEIC".into(),
            title: Some("Pont des Arts".into()),
            taken: DateTime::parse_from_rfc3339("2019-05-03T18:27:59+02:00").ok(),
            latitude: Some(48.8583),
            longitude: Some(2.3375),
            keywords: vec!["travel".into(), "bridge".into()],
            albums: vec!["Paris 2019".into()],
            labels: vec!["Outdoor".into(), "Water".into()],
            people: vec!["Ann".into()],
            camera: Camera {
                make: Some("Apple".into()),
                model: Some("iPhone 15 Pro".into()),
                lens: Some("iPhone 15 Pro back camera 6.86mm f/1.78".into()),
            },
            place: Place {
                name: Some("Louvre".into()),
                city: Some("Paris".into()),
                state: Some("Île-de-France".into()),
                country: Some("FR".into()),
            },
            ..Default::default()
        }
    }

    /// The asset with its taken date read in `zone`, for rendering paths.
    pub fn zoned(&self, zone: DateZone) -> Asset {
        let mut a = self.clone();
        a.taken = a.taken.map(|t| match zone {
            DateZone::Asset => t,
            DateZone::Local => t.with_timezone(&chrono::Local).fixed_offset(),
            DateZone::Utc => t.with_timezone(&Utc).fixed_offset(),
            DateZone::Fixed(o) => t.with_timezone(&o),
        });
        a
    }

    pub fn fill_place(&mut self) {
        if let (true, Some(lat), Some(lon)) = (
            self.place == Place::default(),
            self.latitude,
            self.longitude,
        ) {
            self.place = geo::nearest(lat, lon);
        }
    }

    pub fn stem(&self) -> &str {
        Path::new(&self.filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&self.uuid)
    }

    pub fn field(&self, name: &str, idx: usize) -> Option<String> {
        let date = |f: &str| self.taken.map(|t| t.format(f).to_string());
        match name {
            "take.year" => date("%Y"),
            "take.month" => date("%m"),
            "take.day" => date("%d"),
            "take.date" => date("%Y-%m-%d"),
            "place.name" => self.place.name.clone(),
            "place.city" => self.place.city.clone(),
            "place.state" => self.place.state.clone(),
            "place.country" => self.place.country.clone(),
            "keyword" => self.keywords.get(idx).cloned(),
            "album" => self.albums.get(idx).cloned(),
            "label" => self.labels.get(idx).cloned(),
            "people" => self.people.get(idx).cloned(),
            "camera.make" => self.camera.make.clone(),
            "camera.model" => self.camera.model.clone(),
            "camera.lens" => self.camera.lens.clone(),
            "title" => self.title.clone(),
            "uuid" => Some(self.uuid.clone()),
            "stem" => Some(self.stem().to_owned()),
            "type" => Some(if self.video { "video" } else { "photo" }.into()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_conjunctively() {
        let mut a = Asset::sample();
        a.favorite = true;
        let mut f = Filter::default();
        assert!(f.matches(&a));
        f.media = Media::Videos;
        assert!(!f.matches(&a));
        f.media = Media::Photos;
        f.from = Some(NaiveDate::from_ymd_opt(2019, 1, 1).unwrap());
        f.to = Some(NaiveDate::from_ymd_opt(2019, 12, 31).unwrap());
        f.albums = vec!["paris 2019".into()];
        f.keywords = vec!["Bridge".into(), "x".into()];
        assert!(f.matches(&a));
        f.to = Some(NaiveDate::from_ymd_opt(2019, 5, 2).unwrap());
        assert!(!f.matches(&a));
        a.taken = None;
        assert!(f.matches(&a), "undated assets pass a date range");
        f.favorites = true;
        a.favorite = false;
        assert!(!f.matches(&a));
    }

    #[test]
    fn zones_shift_the_folder_date() {
        let a = Asset::sample();
        assert_eq!(a.field("take.date", 0).as_deref(), Some("2019-05-03"));
        let late = Asset {
            taken: DateTime::parse_from_rfc3339("2019-05-03T00:30:00+02:00").ok(),
            ..Asset::sample()
        };
        assert_eq!(
            late.zoned(DateZone::Utc).field("take.date", 0).as_deref(),
            Some("2019-05-02")
        );
        assert_eq!(
            late.zoned("-07:00".parse().unwrap())
                .field("take.date", 0)
                .as_deref(),
            Some("2019-05-02")
        );
        assert_eq!(
            late.zoned(DateZone::Asset).field("take.date", 0).as_deref(),
            Some("2019-05-03")
        );
    }

    #[test]
    fn date_zone_parses_and_displays() {
        assert!("mars".parse::<DateZone>().is_err());
        assert_eq!("+02:00".parse::<DateZone>().unwrap().to_string(), "+02:00");
    }

    #[test]
    fn every_field_resolves_for_the_sample() {
        let a = Asset::sample();
        for f in FIELDS {
            assert!(a.field(f.name, 0).is_some(), "{}", f.name);
        }
    }
}
