pub mod library;
pub mod photokit;
mod place;

use super::{Resource, Source};
use crate::export::hash_file;
use crate::photo::Asset;
use anyhow::Result;
use std::path::Path;

pub(crate) struct PhotoKit;

impl Source for PhotoKit {
    fn resources(&self, _asset: &Asset, local_id: &str) -> Result<Vec<Resource>> {
        photokit::resources(local_id)
    }
    fn write(&self, local_id: &str, index: usize, dest: &Path) -> Result<String> {
        photokit::write_resource(local_id, index, dest)?;
        hash_file(dest)
    }
}
