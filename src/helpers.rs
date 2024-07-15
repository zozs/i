use std::io::Result;
use std::path::PathBuf;

use super::{get_base_dir, get_thumbnail_dir, Opt};

pub fn filename_path(filename: &str, opt: &Opt) -> Result<PathBuf> {
    Ok(get_base_dir(opt)?.join(sanitize_filename::sanitize(filename)))
}

pub fn thumbnail_filename_path(filename: &str, opt: &Opt) -> Result<PathBuf> {
    Ok(get_thumbnail_dir(opt)?.join(sanitize_filename::sanitize(filename)))
}
