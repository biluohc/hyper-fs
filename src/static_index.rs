use futures::future::{self, FutureResult};
use hyper::{header, Error, Method, StatusCode};
use hyper::server::{Request, Response, Service};
#[allow(unused_imports)]
use walkdir::{DirEntry, Error as WalkDirErr, WalkDir};

use super::{ExceptionCatcher, ExceptionHandler};
use super::Config;

#[allow(unused_imports)]
use std::io::{self, ErrorKind as IoErrorKind};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

// use Template engine? too heavy...
/// Simple html list the name of every entry for a index
pub struct StaticIndex<EH = ExceptionHandler> {
    index: PathBuf,
    config: Arc<Config>,
    handler: EH,
}

impl StaticIndex<ExceptionHandler> {
    pub fn new<P: Into<PathBuf>>(index: P, config: Arc<Config>) -> Self {
        Self::with_handler(index, config, ExceptionHandler::default())
    }
}

impl<EH: ExceptionCatcher> StaticIndex<EH> {
    pub fn with_handler<P: Into<PathBuf>>(index: P, config: Arc<Config>, handler: EH) -> Self {
        Self {
            index: index.into(),
            config: config,
            handler: handler,
        }
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
}
impl<EH> Service for StaticIndex<EH>
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
        // io error
        let html = match render_html(&self.index, req.path(), self.config()) {
            Ok(html) => html,
            Err(e) => {
                self.handler.catch(e);
                return self.handler.call(req);
            }
        };
        // todo: 304

        let mut res = Response::new().with_header(header::ContentLength(html.len() as u64));
        // response body  stream
        match *req.method() {
            Method::Head => {}
            Method::Get => {
                res.set_body(html);
            }
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
<title>Directory listing for {}</title>
</head><body><h1>Directory listing for {}</h1><hr><ul>",
        path, path
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
