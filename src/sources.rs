pub mod apple_photos;
pub mod dropbox;
pub mod onedrive;
pub mod takeout;

use crate::photo::Asset;
use anyhow::Result;
use std::path::Path;

const IMAGES: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "tif", "tiff", "dng", "cr2", "cr3", "nef",
    "arw", "raf", "orf", "rw2",
];
const VIDEOS: &[&str] = &["mp4", "mov", "m4v", "avi", "3gp", "mkv", "webm", "mpg"];

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub fn is_media(name: &str) -> bool {
    let e = extension(name);
    IMAGES.contains(&e.as_str()) || VIDEOS.contains(&e.as_str())
}

pub fn is_video(name: &str) -> bool {
    VIDEOS.contains(&extension(name).as_str())
}

pub trait Source: Sync {
    fn resources(&self, asset: &Asset, local_id: &str) -> Result<Vec<Resource>>;
    fn write(&self, local_id: &str, index: usize, dest: &Path) -> Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Original,
    Alternate,
    Edited,
    Adjustments,
    PairedVideo,
    EditedPairedVideo,
}

impl Kind {
    pub fn wanted(self, cfg: &crate::config::Config) -> bool {
        match self {
            Kind::Original | Kind::Alternate | Kind::PairedVideo => true,
            Kind::Edited | Kind::EditedPairedVideo | Kind::Adjustments => cfg.edits,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Original => "original",
            Kind::Alternate => "alternate",
            Kind::Edited => "edited",
            Kind::Adjustments => "adjustments",
            Kind::PairedVideo => "paired_video",
            Kind::EditedPairedVideo => "edited_paired_video",
        }
    }

    pub fn file_name(self, stem: &str, resource_filename: &str) -> String {
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

#[derive(Debug, Clone)]
pub struct Resource {
    pub index: usize,
    pub kind: Kind,
    pub filename: String,
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
}
