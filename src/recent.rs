use askama::Template;
use askama_web::WebTemplate;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use chrono::offset::Local;
use chrono::{DateTime, Datelike};
use serde::Deserialize;
use std::cmp::Ordering;
use std::io;
use std::{fs, fs::DirEntry};

use crate::WebError;

use super::{Opt, get_base_dir};

struct DirEntryModTimePair {
    dir_entry: DirEntry,
    mod_time: DateTime<Local>,
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
            recents.push(RecentEntry {
                timestamp: entry.mod_time.format("%Y-%m-%d %T").to_string(),
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

async fn recent<DF, S>(
    opt: &Opt,
    page: usize,
    date_filter: DF,
    sorter: S,
) -> Result<impl IntoResponse + use<DF, S>, WebError>
where
    DF: Fn(&DateTime<Local>) -> bool,
    S: Fn(&DirEntryModTimePair, &DirEntryModTimePair) -> Ordering,
{
    let mut files = Vec::new();

    let base_dir = get_base_dir(opt)?;
    visit_dirs(&base_dir, &mut files)?;

    // note the order of the partial_cmp
    //files.sort_by(|a, b| b.mod_time.partial_cmp(&a.mod_time).unwrap());
    files.sort_by(sorter);

    let recent_files = opt.recents;
    let pagination = build_pagination(files.len(), recent_files, page);
    let latest_n_files: Vec<&DirEntryModTimePair> = files
        .iter()
        .filter(|x| date_filter(&x.mod_time))
        .skip(page * recent_files)
        .take(recent_files)
        .collect();

    build_recent_html_page(
        &latest_n_files,
        base_dir.to_string_lossy().len() + 1,
        opt,
        pagination,
    )
}

fn make_filter_year(year: i32) -> impl Fn(&DateTime<Local>) -> bool {
    move |a| a.year() == year
}

fn make_filter_year_month(year: i32, month: u32) -> impl Fn(&DateTime<Local>) -> bool {
    move |a| a.year() == year && a.month() == month
}

fn date_sorter(a: &DirEntryModTimePair, b: &DirEntryModTimePair) -> Ordering {
    b.mod_time.partial_cmp(&a.mod_time).unwrap()
}

fn size_sorter(a: &DirEntryModTimePair, b: &DirEntryModTimePair) -> Ordering {
    b.dir_entry
        .metadata()
        .unwrap()
        .len()
        .partial_cmp(&a.dir_entry.metadata().unwrap().len())
        .unwrap()
}

pub async fn recent_pagination(
    State(opt): State<Opt>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, WebError> {
    recent(&opt, pagination.page, |_| true, date_sorter).await
}

pub async fn recent_pagination_size(
    State(opt): State<Opt>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, WebError> {
    recent(&opt, pagination.page, |_| true, size_sorter).await
}

pub async fn recent_pagination_year(
    Path((year,)): Path<(i32,)>,
    State(opt): State<Opt>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, WebError> {
    recent(&opt, pagination.page, make_filter_year(year), date_sorter).await
}

pub async fn recent_pagination_year_size(
    Path((year,)): Path<(i32,)>,
    State(opt): State<Opt>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, WebError> {
    recent(&opt, pagination.page, make_filter_year(year), size_sorter).await
}

pub async fn recent_pagination_year_month(
    Path((year, month)): Path<(i32, u32)>,
    State(opt): State<Opt>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, WebError> {
    recent(
        &opt,
        pagination.page,
        make_filter_year_month(year, month),
        date_sorter,
    )
    .await
}

pub async fn recent_pagination_year_month_size(
    Path((year, month)): Path<(i32, u32)>,
    State(opt): State<Opt>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse, WebError> {
    recent(
        &opt,
        pagination.page,
        make_filter_year_month(year, month),
        size_sorter,
    )
    .await
}

// Inspired by first example here https://doc.rust-lang.org/std/fs/fn.read_dir.html
fn visit_dirs(dir: &std::path::Path, files: &mut Vec<DirEntryModTimePair>) -> io::Result<()> {
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
                    mod_time: mod_time.into(),
                });
            }
        }
    }

    Ok(())
}
