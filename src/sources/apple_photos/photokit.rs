use super::super::{Kind, Resource};
use anyhow::{Result, anyhow, bail};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSError, NSString, NSURL};
use objc2_photos::{
    PHAccessLevel, PHAsset, PHAssetResource, PHAssetResourceManager, PHAssetResourceRequestOptions,
    PHAssetResourceType, PHAuthorizationStatus, PHPhotoLibrary,
};
use std::path::Path;
use std::sync::mpsc;

fn kind(t: PHAssetResourceType) -> Option<Kind> {
    Some(match t {
        PHAssetResourceType::Photo | PHAssetResourceType::Video | PHAssetResourceType::Audio => {
            Kind::Original
        }
        PHAssetResourceType::AlternatePhoto => Kind::Alternate,
        PHAssetResourceType::FullSizePhoto | PHAssetResourceType::FullSizeVideo => Kind::Edited,
        PHAssetResourceType::AdjustmentData => Kind::Adjustments,
        PHAssetResourceType::PairedVideo => Kind::PairedVideo,
        PHAssetResourceType::FullSizePairedVideo => Kind::EditedPairedVideo,
        _ => return None,
    })
}

pub fn authorize() -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |status: PHAuthorizationStatus| {
        let _ = tx.send(status);
    });
    unsafe {
        PHPhotoLibrary::requestAuthorizationForAccessLevel_handler(
            PHAccessLevel::ReadWrite,
            &handler,
        )
    };
    match rx.recv()? {
        PHAuthorizationStatus::Authorized => Ok(()),
        s => bail!("photo library access not granted (status {})", s.0),
    }
}

pub fn list_assets() -> Vec<(String, String)> {
    let result = unsafe { PHAsset::fetchAssetsWithOptions(None) };
    (0..unsafe { result.count() })
        .map(|i| {
            let id = unsafe { result.objectAtIndex(i).localIdentifier() }.to_string();
            let uuid = id.split('/').next().unwrap_or("").to_owned();
            (uuid, id)
        })
        .collect()
}

fn fetch(local_id: &str) -> Result<Retained<NSArray<PHAssetResource>>> {
    let ids = NSArray::from_retained_slice(&[NSString::from_str(local_id)]);
    let asset =
        unsafe { PHAsset::fetchAssetsWithLocalIdentifiers_options(&ids, None).firstObject() }
            .ok_or_else(|| anyhow!("asset {local_id} not found"))?;
    Ok(unsafe { PHAssetResource::assetResourcesForAsset(&asset) })
}

pub fn resources(local_id: &str) -> Result<Vec<Resource>> {
    Ok(fetch(local_id)?
        .iter()
        .enumerate()
        .filter_map(|(index, r)| {
            let kind = kind(unsafe { r.r#type() })?;
            Some(Resource {
                index,
                kind,
                filename: unsafe { r.originalFilename() }.to_string(),
            })
        })
        .collect())
}

pub fn write_resource(local_id: &str, index: usize, dest: &Path) -> Result<()> {
    let resources = fetch(local_id)?;
    if index >= resources.len() {
        bail!("resource {index} of {local_id} missing");
    }
    let resource = resources.objectAtIndex(index);
    let options = unsafe { PHAssetResourceRequestOptions::new() };
    unsafe { options.setNetworkAccessAllowed(true) };
    let url = NSURL::fileURLWithPath(&NSString::from_str(&dest.to_string_lossy()));
    let (tx, rx) = mpsc::channel();
    let completion = RcBlock::new(move |err: *mut NSError| {
        let msg = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string());
        let _ = tx.send(msg);
    });
    unsafe {
        PHAssetResourceManager::defaultManager()
            .writeDataForAssetResource_toFile_options_completionHandler(
                &resource,
                &url,
                Some(&options),
                &completion,
            )
    };
    match rx.recv()? {
        None => Ok(()),
        Some(msg) => bail!("{msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_resource_types() {
        assert_eq!(kind(PHAssetResourceType::Photo), Some(Kind::Original));
        assert_eq!(
            kind(PHAssetResourceType::AdjustmentData),
            Some(Kind::Adjustments)
        );
        assert_eq!(kind(PHAssetResourceType::AdjustmentBasePhoto), None);
        assert_eq!(kind(PHAssetResourceType::PhotoProxy), None);
    }

    #[test]
    #[ignore]
    fn downloads_first_original() {
        authorize().unwrap();
        let assets = list_assets();
        let (uuid, id) = assets.first().expect("library is empty");
        let rs = resources(id).unwrap();
        let original = rs.iter().find(|r| r.kind == Kind::Original).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join(&original.filename);
        write_resource(id, original.index, &dest).unwrap();
        let len = std::fs::metadata(&dest).unwrap().len();
        println!(
            "{uuid} {} {len} bytes, {} resources",
            original.filename,
            rs.len()
        );
        assert!(len > 0);
    }
}
