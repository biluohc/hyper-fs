use hyper::{header, Error, Method, StatusCode};
use hyper::server::{Request, Response};

#[allow(unused_imports)]
use walkdir::{DirEntry, Error as WalkDirErr, WalkDir};
use url::percent_encoding::{percent_decode, utf8_percent_encode, DEFAULT_ENCODE_SET};
use futures_cpupool::{CpuFuture, CpuPool};
use futures::{Future, Poll};

use super::{Exception, ExceptionHandler, ExceptionHandlerService};
use super::{Config, FutureObject};

#[allow(unused_imports)]
use std::io::{self, ErrorKind as IoErrorKind};
use std::path::PathBuf;
use std::{mem, time};
use std::fs;

struct Inner<C, EH = ExceptionHandler> {
    index: PathBuf,
    headers: Option<header::Headers>,
    config: C,
    handler: EH,
}
impl<C, EH> Inner<C, EH>
where
    C: AsRef<Config>,
    EH: ExceptionHandlerService,
{
    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }
}

// use Template engine? too heavy...
/// Static Index: Simple html list the name of every entry for a index
pub struct StaticIndex<C, EH = ExceptionHandler> {
    inner: Option<Inner<C, EH>>,
    content: Option<CpuFuture<Response, Error>>,
}

impl<C> StaticIndex<C, ExceptionHandler>
where
    C: AsRef<Config> + Send,
{
    pub fn new<P: Into<PathBuf>>(index: P, config: C) -> Self {
        Self::with_handler(index, config, ExceptionHandler::default())
    }
}

impl<C, EH> StaticIndex<C, EH>
where
    C: AsRef<Config> + Send,
    EH: ExceptionHandlerService + Send + 'static,
{
    pub fn with_handler<P: Into<PathBuf>>(index: P, config: C, handler: EH) -> Self {
        let inner = Inner {
            index: index.into(),
            headers: None,
            config: config,
            handler: handler,
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
}

impl<C, EH> Future for StaticIndex<C, EH> {
    type Item = Response;
    type Error = Error;
    fn poll(&mut self) -> Poll<Response, Error> {
        self.content
            .as_mut()
            .expect("Poll a empty StaticIndex(NotInit/AlreadyConsume)")
            .poll()
    }
}

impl<C, EH> StaticIndex<C, EH>
where
    C: AsRef<Config> + Send + 'static,
    EH: ExceptionHandlerService + Send + 'static,
{
    pub fn call(mut self, pool: &CpuPool, req: Request) -> FutureObject {
        let mut inner = mem::replace(&mut self.inner, None).expect("Call twice");
        self.content = Some(pool.spawn_fn(move || inner.call(req)));
        Box::new(self)
    }
}

impl<C, EH> Inner<C, EH>
where
    C: AsRef<Config>,
    EH: ExceptionHandlerService,
{
    fn call(&mut self, req: Request) -> Result<Response, Error> {
        let mut headers = mem::replace(&mut self.headers, None).unwrap_or_else(header::Headers::new);
        if self.config().cache_secs != 0 {
            headers.set(header::CacheControl(vec![
                header::CacheDirective::Public,
                header::CacheDirective::MaxAge(self.config().cache_secs),
            ]));
        }
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return self.handler.call(Exception::Method, req),
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
            return Ok(Response::new()
                .with_status(StatusCode::MovedPermanently)
                .with_headers(headers));
        }
        if !self.config().get_show_index() {
            match fs::read_dir(&self.index) {
                Ok(_) => {
                    return Ok(Response::new().with_headers(headers));
                }
                Err(e) => {
                    return self.handler.call(e, req);
                }
            }
        }
        // HTTP Last-Modified
        let metadata = match self.index.as_path().metadata() {
            Ok(m) => m,
            Err(e) => {
                return self.handler.call(e, req);
            }
        };
        let last_modified = match metadata.modified() {
            Ok(time) => time,
            Err(e) => {
                return self.handler.call(e, req);
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
        if let Some(&header::IfNoneMatch::Items(ref etags)) = req.headers().get() {
            if !etags.is_empty() && etag == etags[0] {
                return Ok(Response::new()
                    .with_headers(headers)
                    .with_status(StatusCode::NotModified));
            }
        }

        // io error
        let html = match render_html(&self.index, req.path(), self.config()) {
            Ok(html) => html,
            Err(e) => {
                return self.handler.call(e, req);
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
        Ok(res)
    }
}

fn render_html(index: &PathBuf, path: &str, config: &Config) -> io::Result<String> {
    let path_dec = percent_decode(path.as_bytes()).decode_utf8().unwrap();
    debug!("\nreq: {:?}\ndec: {:?}", path, path_dec);
    let mut html = format!(
        "
<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">
<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">
<title>Index listing for {}</title>
</head><body><h1>Index listing for  <a href=\"{}../\">{}</a></h1><hr><ul>",
        path_dec, path, path_dec
    );

    let mut walker = WalkDir::new(index).min_depth(1).max_depth(1);
    if config.get_follow_links() {
        walker = walker.follow_links(true);
    }
    if config.get_hide_entry() {
        for entry in walker.into_iter().filter_entry(|e| !is_hidden(e)) {
            entries_render(entry?, &mut html);
        }
    } else {
        for entry in walker {
            entries_render(entry?, &mut html);
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
fn entries_render(entry: DirEntry, html: &mut String) {
    let mut name = entry.file_name().to_string_lossy().into_owned();
    if entry.file_type().is_dir() {
        name.push('/');
    }
    let li = format!(
        "<li><a href=\"{}\">{}</a></li>",
        utf8_percent_encode(&name, DEFAULT_ENCODE_SET),
        name
    );
    html.push_str(&li);
}
