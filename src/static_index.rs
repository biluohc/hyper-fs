use futures::future::{self, FutureResult};
use hyper::{header, Error, Method, StatusCode};
use hyper::server::{Request, Response, Service};
#[allow(unused_imports)]
use walkdir::{DirEntry, Error as WalkDirErr, WalkDir};

use super::{ExceptionCatcher, ExceptionHandler};
use super::Config;

#[allow(unused_imports)]
use std::io::{self, ErrorKind as IoErrorKind};
use std::path::PathBuf;
use std::time;
use std::fs;

// use Template engine? too heavy...
/// Simple html list the name of every entry for a index
pub struct StaticIndex<C, EH = ExceptionHandler> {
    index: PathBuf,
    config: C,
    handler: EH,
}

impl<C: AsRef<Config>> StaticIndex<C, ExceptionHandler> {
    pub fn new<P: Into<PathBuf>>(index: P, config: C) -> Self {
        Self::with_handler(index, config, ExceptionHandler::default())
    }
}

impl<C: AsRef<Config>, EH: ExceptionCatcher> StaticIndex<C, EH> {
    pub fn with_handler<P: Into<PathBuf>>(index: P, config: C, handler: EH) -> Self {
        Self {
            index: index.into(),
            config: config,
            handler: handler,
        }
    }
    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }
}
impl<C: AsRef<Config>, EH> Service for StaticIndex<C, EH>
where
    EH: ExceptionCatcher + Service<Request = Request, Response = Response, Error = Error, Future = FutureResult<Response, Error>>,
{
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    fn call(&self, req: Request) -> Self::Future {
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return self.handler.call(req),
        }
        // 301
        if !req.path().ends_with('/') {
            let mut new_path = req.path().to_owned();
            new_path.push('/');
            if let Some(query) = req.query() {
                new_path.push('?');
                new_path.push_str(query);
            }
            return future::ok(
                Response::new()
                    .with_status(StatusCode::MovedPermanently)
                    .with_header(header::Location::new(new_path)),
            );
        }
        if !self.config().get_show_index() {
            match fs::read_dir(&self.index) {
                Ok(_) => {
                    return future::ok(Response::new());
                }
                Err(e) => {
                    self.handler.catch(e);
                    return self.handler.call(req);
                }
            }
        }
        // HTTP Last-Modified
        let metadata = match self.index.as_path().metadata() {
            Ok(m) => m,
            Err(e) => {
                self.handler.catch(e);
                return self.handler.call(req);
            }
        };
        let last_modified = match metadata.modified() {
            Ok(time) => time,
            Err(e) => {
                self.handler.catch(e);
                return self.handler.call(req);
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
            if !etags.is_empty() {
                debug!(
                    "304: {}\nfs\n{:?}\nhttp\n{:?}",
                    etag == etags[0],
                    etag,
                    etags[0]
                );
                if etag == etags[0] {
                    return future::ok(Response::new().with_status(StatusCode::NotModified));
                }
            }
        }

        // io error
        let html = match render_html(&self.index, req.path(), self.config()) {
            Ok(html) => html,
            Err(e) => {
                self.handler.catch(e);
                return self.handler.call(req);
            }
        };

        // response Header
        let mut res = Response::new()
            .with_header(header::ContentLength(html.len() as u64))
            .with_header(header::LastModified(http_last_modified))
            .with_header(header::ETag(etag));

        if self.config().cache_secs != 0 {
            res.headers_mut().set(header::CacheControl(vec![
                header::CacheDirective::Public,
                header::CacheDirective::MaxAge(self.config().cache_secs),
            ]));
        }

        // response body  stream
        match *req.method() {
            Method::Get => {
                res.set_body(html);
            }
            Method::Head => {}
            _ => unreachable!(),
        }
        future::ok(res)
    }
}

fn render_html(index: &PathBuf, path: &str, config: &Config) -> io::Result<String> {
    let mut html = format!(
        "
<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">
<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">
<title>Index listing for {}</title>
</head><body><h1>Index listing for  <a href=\"{}../\">{}</a></h1><hr><ul>",
        path, path, path
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
    let li = format!("<li><a href=\"{}\">{}</a></li>", name, name);
    html.push_str(&li);
}
