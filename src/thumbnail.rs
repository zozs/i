use anyhow::Result;
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
