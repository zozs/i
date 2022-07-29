use anyhow::{anyhow, Result};
use std::path::Path;

use super::Opt;

/**
 * Tries to generate a thumbnail of the given filename. Returns false if it wasn't an image.
 */
pub fn generate_thumbnail<P>(path: P, thumb_path: P, opt: &Opt) -> Result<bool>
where
    P: AsRef<Path>,
{
    if let Ok(img) = image::open(path) {
        let thumb = img.resize_to_fill(
            opt.thumbnail_size,
            opt.thumbnail_size,
            image::imageops::Triangle,
        );
        thumb.save(thumb_path)?;

        return Ok(true);
    }

    Ok(false)
}

/**
 * Returns relative url to thumbnail, or a placeholder image if it doesn't exist
 */
pub fn get_thumbnail_url<P: AsRef<Path>>(path: P, opt: &Opt) -> Result<String> {
    let thumbnail_path = super::get_thumbnail_dir(opt)?.join(&path);
    return if thumbnail_path.exists() {
        let url = std::path::Path::new(crate::THUMBNAIL_SUBDIR);
        url.join(&path)
            .into_os_string()
            .into_string()
            .map_err(|e| anyhow!("path error {:?}", e))
    } else {
        Ok("/recent/placeholder.png".to_string())
    };
}
