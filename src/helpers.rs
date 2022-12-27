use actix_web::Error;
use std::path::PathBuf;

use super::{get_base_dir, get_thumbnail_dir, Opt};

pub fn filename_path(filename: &str, opt: &Opt) -> Result<PathBuf, Error> {
    Ok(get_base_dir(opt)?.join(sanitize_filename::sanitize(filename)))
}

pub fn thumbnail_filename_path(filename: &str, opt: &Opt) -> Result<PathBuf, Error> {
    Ok(get_thumbnail_dir(opt)?.join(sanitize_filename::sanitize(filename)))
}
