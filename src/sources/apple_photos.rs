pub mod library;
#[cfg(target_os = "macos")]
pub mod photokit;

#[cfg(target_os = "macos")]
use super::{Resource, Source};
#[cfg(target_os = "macos")]
use crate::export::hash_file;
#[cfg(target_os = "macos")]
use crate::photo::Asset;
#[cfg(target_os = "macos")]
use anyhow::Result;
#[cfg(target_os = "macos")]
use std::path::Path;

#[cfg(target_os = "macos")]
pub struct PhotoKit;

#[cfg(target_os = "macos")]
impl Source for PhotoKit {
    fn resources(&self, _asset: &Asset, local_id: &str) -> Result<Vec<Resource>> {
        photokit::resources(local_id)
    }
    fn write(&self, local_id: &str, index: usize, dest: &Path) -> Result<String> {
        photokit::write_resource(local_id, index, dest)?;
        hash_file(dest)
    }
}
