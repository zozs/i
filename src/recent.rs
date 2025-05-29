use askama::Template;
use askama_web::WebTemplate;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use chrono::DateTime;
use chrono::offset::Local;
use serde::Deserialize;
use std::io;
use std::path::Path;
use std::time::SystemTime;
use std::{fs, fs::DirEntry};

use crate::WebError;

use super::{Opt, get_base_dir};

struct DirEntryModTimePair {
    dir_entry: DirEntry,
    mod_time: SystemTime,
}

struct RecentEntry {
    thumbnail_url: String,
    timestamp: String,
    url: String,
}

#[derive(Deserialize)]
pub struct Pagination {
    #[serde(default)]
    page: usize,
}

pub struct PaginationBar {
    prev: Option<usize>,
    next: Option<usize>,
    pages: Vec<PaginationNode>,
}

enum PaginationNode {
    Ellipsis,
    Current(usize),
    Page(usize),
}

#[derive(Template, WebTemplate)]
#[template(path = "recent.html")]
struct RecentTemplate {
    recents: Vec<RecentEntry>,
    pagination: PaginationBar,
}

fn is_current(page: i64, current: usize) -> PaginationNode {
    let page = page as usize;
    if page == current {
        PaginationNode::Current(page)
    } else {
        PaginationNode::Page(page)
    }
}

fn build_pagination(total: usize, per_page: usize, current: usize) -> PaginationBar {
    let max = total.div_ceil(per_page) as i64; // max number of pages
    let current = current + 1; // 1-indexed calculations in algorithm
    // Inspired by: https://www.zacfukuda.com/blog/pagination-algorithm
    let prev = (current > 1).then(|| current - 1);
    let next = ((current as i64) < max).then(|| current + 1);
    let mut pages = vec![is_current(1, current)];
    if current == 1 && max == 1 {
        return PaginationBar { next, prev, pages };
    }
    if current > 4 {
        pages.push(PaginationNode::Ellipsis);
    }
    let r = 2;
    let r1 = current as i64 - r;
    let r2 = current as i64 + r;
    let istart = if r1 > 2 { r1 } else { 2 };
    for i in istart..=max.min(r2) {
        pages.push(is_current(i, current));
    }

    if r2 + 1 < max {
        pages.push(PaginationNode::Ellipsis);
    }
    if r2 < max {
        pages.push(is_current(max, current));
    }

    PaginationBar { next, prev, pages }
}

fn build_recent_html_page(
    files: &[&DirEntryModTimePair],
    prefix_length: usize,
    opt: &Opt,
    pagination: PaginationBar,
) -> Result<impl IntoResponse + use<>, WebError> {
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
                thumbnail_url: super::thumbnail::get_thumbnail_url(path, opt)?,
            });
        }
    }

    let template = RecentTemplate {
        recents,
        pagination,
    };
    Ok(template)
}

async fn recent(opt: &Opt, page: usize) -> Result<impl IntoResponse + use<>, WebError> {
    let mut files = Vec::new();

    let base_dir = get_base_dir(opt)?;
    visit_dirs(&base_dir, &mut files)?;

    // note the order of the partial_cmp
    files.sort_by(|a, b| b.mod_time.partial_cmp(&a.mod_time).unwrap());

    let recent_files = opt.recents;
    let pagination = build_pagination(files.len(), recent_files, page);
    let latest_n_files: Vec<&DirEntryModTimePair> = files
        .iter()
        .skip(page * recent_files)
        .take(recent_files)
        .collect();

    build_recent_html_page(
        &latest_n_files,
        base_dir.to_string_lossy().len() + 1,
        opt,
        pagination,
    )
    // + 1 for the dir separator
}

pub async fn recent_pagination(
    State(opt): State<Opt>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, WebError> {
    recent(&opt, pagination.page).await
}

// Inspired by first example here https://doc.rust-lang.org/std/fs/fn.read_dir.html
fn visit_dirs(dir: &Path, files: &mut Vec<DirEntryModTimePair>) -> io::Result<()> {
    // TODO: Check error handling when I know more about error handling in Rust.
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let dir_entry = entry?;
            let path = dir_entry.path();
            if path.is_dir() {
                if !path.ends_with(crate::THUMBNAIL_SUBDIR) {
                    visit_dirs(&path, files)?
                }
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
