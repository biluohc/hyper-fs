use hyper::{header, Method, StatusCode};
use hyper::server::{Request, Response};

use walkdir::{DirEntry, WalkDir};
use url::percent_encoding::percent_encode_byte;
use futures_cpupool::{CpuFuture, CpuPool};
use futures::{Future, Poll};

use super::{Config, Error, FutureObject};

use std::path::PathBuf;
use std::{mem, time};
use std::fs;
use std::io;

struct Inner<C> {
    title: String,
    path: PathBuf,
    headers: Option<header::Headers>,
    config: C,
}

impl<C> Inner<C>
where
    C: AsRef<Config>,
{
    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }
}

// use Template engine? too heavy...
/// Static Index: Simple html list the name of every entry for a index
pub struct StaticIndex<C> {
    inner: Option<Inner<C>>,
    content: Option<CpuFuture<(Response, Request), (Error, Request)>>,
}

impl<C> StaticIndex<C>
where
    C: AsRef<Config> + Send + 'static,
{
    pub fn new<S: Into<String>, P: Into<PathBuf>>(title: S, path: P, config: C) -> Self {
        let inner = Inner {
            title: title.into(),
            path: path.into(),
            headers: None,
            config: config,
        };
        Self {
            inner: Some(inner),
            content: None,
        }
    }
    /// You can set the init `Haeders`
    ///
    /// Warning: not being covered by inner code.
    pub fn headers_mut(&mut self) -> &mut Option<header::Headers> {
        &mut self.inner.as_mut().unwrap().headers
    }
    pub fn call(mut self, pool: &CpuPool, req: Request) -> FutureObject {
        let mut inner = mem::replace(&mut self.inner, None).expect("Call twice");
        self.content = Some(pool.spawn_fn(move || inner.call(req)));
        Box::new(self)
    }
}

#[doc(hidden)]
impl<C> Future for StaticIndex<C> {
    type Item = (Response, Request);
    type Error = (Error, Request);
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.content
            .as_mut()
            .expect("Poll a empty StaticIndex(NotInit/AlreadyConsume)")
            .poll()
    }
}

impl<C> Inner<C>
where
    C: AsRef<Config>,
{
    fn call(&mut self, req: Request) -> Result<(Response, Request), (Error, Request)> {
        let mut headers = mem::replace(&mut self.headers, None).unwrap_or_else(header::Headers::new);
        if *self.config().get_cache_secs() != 0 {
            headers.set(header::CacheControl(vec![
                header::CacheDirective::Public,
                header::CacheDirective::MaxAge(*self.config().get_cache_secs()),
            ]));
        }
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return Err((Error::Method, req)),
        }
        // 301
        if !req.path().ends_with('/') {
            let mut new_path = req.path().to_owned();
            new_path.push('/');
            if let Some(query) = req.query() {
                new_path.push('?');
                new_path.push_str(query);
            }
            headers.set(header::Location::new(new_path));
            return Ok((
                Response::new()
                    .with_status(StatusCode::MovedPermanently)
                    .with_headers(headers),
                req,
            ));
        }
        if !self.config().get_show_index() {
            match fs::read_dir(&self.path) {
                Ok(_) => {
                    return Ok((Response::new().with_headers(headers), req));
                }
                Err(e) => {
                    return Err((e.into(), req));
                }
            }
        }
        // HTTP Last-Modified
        let metadata = match self.path.as_path().metadata() {
            Ok(m) => m,
            Err(e) => {
                return Err((e.into(), req));
            }
        };
        let last_modified = match metadata.modified() {
            Ok(time) => time,
            Err(e) => {
                return Err((e.into(), req));
            }
        };
        let delta_modified = last_modified
            .duration_since(time::UNIX_EPOCH)
            .expect("SystemTime::duration_since(UNIX_EPOCH) failed");
        let http_last_modified = header::HttpDate::from(last_modified);
        let etag = header::EntityTag::weak(format!(
            "{:x}-{:x}.{:x}",
            metadata.len(),
            delta_modified.as_secs(),
            delta_modified.subsec_nanos()
        ));

        let mut _304 = false;
        if let Some(&header::IfNoneMatch::Items(ref etags)) = req.headers().get() {
            if !etags.is_empty() && *self.config.as_ref().get_cache_secs() > 0 && etag == etags[0] {
                _304 = true;
            }
        }
        if _304 {
            return Ok((
                Response::new()
                    .with_headers(headers)
                    .with_status(StatusCode::NotModified),
                req,
            ));
        }

        // io error
        let html = match render_html(&self.title, &self.path, req.path(), self.config()) {
            Ok(html) => html,
            Err(e) => {
                return Err((e.into(), req));
            }
        };

        // response Header
        headers.set(header::ContentLength(html.len() as u64));
        headers.set(header::LastModified(http_last_modified));
        headers.set(header::ETag(etag));
        let mut res = Response::new().with_headers(headers);

        // response body  stream
        match *req.method() {
            Method::Get => {
                res.set_body(html);
            }
            Method::Head => {}
            _ => unreachable!(),
        }
        Ok((res, req))
    }
}

fn render_html(title: &str, index: &PathBuf, path: &str, config: &Config) -> io::Result<String> {
    let mut html = format!(
        "
<!DOCTYPE HTML>
<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">
<title>Index listing for {}</title>
</head><body><h1>Index listing for  <a href=\"{}../\">{}</a></h1><hr><ul>",
        title, path, title
    );

    let mut walker = WalkDir::new(index).min_depth(1).max_depth(1);
    if config.get_follow_links() {
        walker = walker.follow_links(true);
    }
    if config.get_hide_entry() {
        for entry in walker.into_iter().filter_entry(|e| !is_hidden(e)) {
            entries_render(&entry?, &mut html);
        }
    } else {
        for entry in walker {
            entries_render(&entry?, &mut html);
        }
    }
    html.push_str("</ul><hr></body></html>");
    Ok(html)
}

#[inline]
fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}
#[inline]
fn entries_render(entry: &DirEntry, html: &mut String) {
    let mut name = entry.file_name().to_string_lossy().into_owned();
    let mut name_dec = name.bytes().map(percent_encode_byte).collect::<String>();
    if entry.file_type().is_dir() {
        name.push('/');
        name_dec.push('/');
    }
    let li = format!("<li><a href=\"{}\">{}</a></li>", name_dec, name);
    html.push_str(&li);
}
