#[cfg(target_os = "macos")]
pub mod apple_photos;
pub mod dropbox;
mod oauth;
pub mod onedrive;
pub mod takeout;

use crate::photo::Asset;
use anyhow::Result;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use std::path::Path;

const IMAGES: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "tif", "tiff", "dng", "cr2", "cr3", "nef",
    "arw", "raf", "orf", "rw2",
];
const VIDEOS: &[&str] = &["mp4", "mov", "m4v", "avi", "3gp", "mkv", "webm", "mpg"];

pub(crate) fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub(crate) fn is_media(name: &str) -> bool {
    let e = extension(name);
    IMAGES.contains(&e.as_str()) || VIDEOS.contains(&e.as_str())
}

pub(crate) fn is_video(name: &str) -> bool {
    VIDEOS.contains(&extension(name).as_str())
}

pub(crate) trait Source: Sync {
    /// The files to export for the asset; by default just its original.
    fn resources(&self, asset: &Asset, _local_id: &str) -> Result<Vec<Resource>> {
        Ok(vec![Resource {
            index: 0,
            kind: Kind::Original,
            filename: asset.filename.clone(),
        }])
    }
    fn write(&self, local_id: &str, index: usize, dest: &Path) -> Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Original,
    Alternate,
    Edited,
    Adjustments,
    PairedVideo,
    EditedPairedVideo,
}

impl Kind {
    const ALL: [Kind; 6] = [
        Kind::Original,
        Kind::Alternate,
        Kind::Edited,
        Kind::Adjustments,
        Kind::PairedVideo,
        Kind::EditedPairedVideo,
    ];

    pub(crate) fn wanted(self, cfg: &crate::config::Config) -> bool {
        match self {
            Kind::Original | Kind::Alternate | Kind::PairedVideo => true,
            Kind::Edited | Kind::EditedPairedVideo | Kind::Adjustments => cfg.edits,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Original => "original",
            Kind::Alternate => "alternate",
            Kind::Edited => "edited",
            Kind::Adjustments => "adjustments",
            Kind::PairedVideo => "paired_video",
            Kind::EditedPairedVideo => "edited_paired_video",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }

    pub(crate) fn file_name(self, stem: &str, resource_filename: &str) -> String {
        let ext = Path::new(resource_filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin");
        match self {
            Kind::Original | Kind::Alternate | Kind::PairedVideo => format!("{stem}.{ext}"),
            Kind::Edited | Kind::EditedPairedVideo => format!("{stem}_edited.{ext}"),
            Kind::Adjustments => format!("{stem}.AAE"),
        }
    }
}

impl ToSql for Kind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.name()))
    }
}

impl FromSql for Kind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Kind> {
        let name = value.as_str()?;
        Kind::parse(name).ok_or_else(|| FromSqlError::Other(format!("unknown kind {name}").into()))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Resource {
    pub(crate) index: usize,
    pub(crate) kind: Kind,
    pub(crate) filename: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_files() {
        assert_eq!(
            Kind::Original.file_name("IMG_1", "IMG_1.HEIC"),
            "IMG_1.HEIC"
        );
        assert_eq!(Kind::Alternate.file_name("IMG_1", "IMG_1.DNG"), "IMG_1.DNG");
        assert_eq!(
            Kind::Edited.file_name("IMG_1", "FullSizeRender.jpeg"),
            "IMG_1_edited.jpeg"
        );
        assert_eq!(
            Kind::Adjustments.file_name("IMG_1", "FullSizeRender.plist"),
            "IMG_1.AAE"
        );
        assert_eq!(
            Kind::EditedPairedVideo.file_name("IMG_1", "x.mov"),
            "IMG_1_edited.mov"
        );
        assert_eq!(Kind::Original.file_name("IMG_1", "noext"), "IMG_1.bin");
    }

    #[test]
    fn parses_names() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.name()), Some(k));
        }
        assert_eq!(Kind::parse("nope"), None);
    }
}
