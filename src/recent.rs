use actix_web::{web, Error, HttpResponse, Responder};
use askama_actix::{Template, TemplateIntoResponse};
use chrono::offset::Local;
use chrono::DateTime;
use std::io;
use std::path::Path;
use std::time::SystemTime;
use std::{fs, fs::DirEntry};

use super::{get_base_dir, Opt};

struct DirEntryModTimePair {
    dir_entry: DirEntry,
    mod_time: SystemTime,
}

struct RecentEntry {
    timestamp: String,
    url: String,
}

#[derive(Template)]
#[template(path = "recent.html")]
struct RecentTemplate<'a> {
    recents: &'a [RecentEntry],
}

fn build_recent_html_page(
    files: &[&DirEntryModTimePair],
    prefix_length: usize,
) -> Result<HttpResponse, Error> {
    // Stringify DirEntryModTimePair
    // TODO: can we make some magic converter Trait to do this outside this function?
    let mut recents: Vec<RecentEntry> = Vec::new();
    for entry in files {
        if let Some(x) = entry.dir_entry.path().to_str() {
            let path = &x[prefix_length..];
            let datetime: DateTime<Local> = entry.mod_time.into();
            recents.push(RecentEntry {
                timestamp: datetime.format("%Y-%m-%d %T").to_string(),
                url: path.to_string(),
            });
        }
    }

    let template = RecentTemplate { recents: &recents };
    template.into_response()
}

pub async fn recent(opt: web::Data<Opt>) -> Result<impl Responder, Error> {
    let mut files = Vec::new();

    let base_dir = get_base_dir(&opt)?.to_string();
    visit_dirs(Path::new(&base_dir), &mut files)?;

    // note the order of the partial_cmp
    files.sort_by(|a, b| b.mod_time.partial_cmp(&a.mod_time).unwrap());

    let n_of_recent_files = opt.recents;
    let latest_n_files: Vec<&DirEntryModTimePair> = files.iter().take(n_of_recent_files).collect();

    build_recent_html_page(&latest_n_files, base_dir.len() + 1) // + 1 for the dir separator
}

// Inspired by first example here https://doc.rust-lang.org/std/fs/fn.read_dir.html
fn visit_dirs(dir: &Path, files: &mut Vec<DirEntryModTimePair>) -> io::Result<()> {
    // TODO: Check error handling when I know more about error handling in Rust.
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let dir_entry = entry?;
            let path = dir_entry.path();
            if path.is_dir() {
                visit_dirs(&path, files)?
            } else {
                let mod_time = match dir_entry.metadata()?.modified() {
                    Ok(n) => n,
                    Err(_) => panic!("SystemTime before UNIX EPOCH!"),
                };

                files.push(DirEntryModTimePair {
                    dir_entry,
                    mod_time,
                });
            }
        }
    }

    Ok(())
}
