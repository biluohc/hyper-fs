use hyper::server::{Request, Response, Service};
use hyper::{header, Error, Method};
use futures_cpupool::CpuPool;
use tokio_core::reactor::Handle;
use url::percent_encoding::percent_decode;

use super::{Exception, ExceptionHandler, ExceptionHandlerService, ExceptionHandlerServiceAsync};
use super::{StaticFile, StaticIndex};
use super::FutureObject;
use super::Config;

use std::path::PathBuf;

/// Static File System
// Todu: full test...
pub struct StaticFs<C, EH = ExceptionHandler> {
    url: String,   // http's base path
    path: PathBuf, // Fs's base path
    handle: Handle,
    pool: CpuPool,
    headers_file: Option<header::Headers>,
    headers_index: Option<header::Headers>,
    config: C,
    handler: EH,
}

impl<C> StaticFs<C, ExceptionHandler>
where
    C: AsRef<Config> + Clone + Send,
{
    pub fn new<U, P>(handle: Handle, pool: CpuPool, url: U, path: P, config: C) -> Self
    where
        U: Into<String>,
        P: Into<PathBuf>,
    {
        Self::with_handler(handle, pool, url, path, config, ExceptionHandler::default())
    }
}

impl<C, EH> StaticFs<C, EH>
where
    C: AsRef<Config> + Clone + Send,
    EH: ExceptionHandlerService + Clone + Send,
{
    pub fn with_handler<U, P>(handle: Handle, pool: CpuPool, url: U, path: P, config: C, handler: EH) -> Self
    where
        U: Into<String>,
        P: Into<PathBuf>,
    {
        Self {
            url: url.into(),
            path: path.into(),
            handle: handle,
            pool: pool,
            handler: handler,
            config: config,
            headers_index: None,
            headers_file: None,
        }
    }
    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }
    pub fn headers_file_mut(&mut self) -> &mut Option<header::Headers> {
        &mut self.headers_file
    }
    pub fn headers_index_mut(&mut self) -> &mut Option<header::Headers> {
        &mut self.headers_index
    }
}

impl<C, EH> Service for StaticFs<C, EH>
where
    C: AsRef<Config> + Clone + Send + 'static,
    EH: ExceptionHandlerService + Clone + Send + 'static,
{
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureObject;
    fn call(&self, req: Request) -> Self::Future {
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return self.handler.call_async(Exception::Method, req),
        }
        debug!(
            "\nurl : {:?}\npath: {:?}\nreqRaw: {:?}\nreqDec: {:?}",
            self.url,
            self.path,
            req.path(),
            route(req.path(), &self.url, &self.path),
        );
        let fspath = match route(req.path(), &self.url, &self.path) {
            Ok(p) => p,
            Err(e) => return self.handler.call_async(e, req),
        };
        let metadata = if self.config().get_follow_links() {
            fspath.metadata()
        } else {
            fspath.symlink_metadata()
        };
        match metadata {
            Ok(md) => {
                let config = self.config.clone();
                if md.is_file() {
                    let mut file_server = StaticFile::with_handler(
                        self.handle.clone(),
                        self.pool.clone(),
                        fspath,
                        config.as_ref().clone(),
                        self.handler.clone(),
                    );
                    if self.headers_file.is_some() {
                        *file_server.headers_mut() = self.headers_file.clone();
                    }
                    file_server.call(&self.pool, req)
                } else if md.is_dir() {
                    let mut index_server = StaticIndex::with_handler(fspath, config.clone(), self.handler.clone());
                    if self.headers_index.is_some() {
                        *index_server.headers_mut() = self.headers_index.clone();
                    }
                    index_server.call(&self.pool, req)
                } else {
                    self.handler.call_async(Exception::Typo, req)
                }
            }
            Err(e) => self.handler.call_async(e, req),
        }
    }
}

fn route(p: &str, base: &str, path: &PathBuf) -> Result<PathBuf, Exception> {
    let mut components = p.split('/')
        .filter(|c| !c.is_empty())
        .map(|c| {
            (
                c,
                true,
                percent_decode(c.as_bytes())
                    .decode_utf8()
                    .unwrap()
                    .into_owned()
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    (0..components.len())
        .into_iter()
        .rev()
        .for_each(|idx| match components[idx].0 {
            "." => {
                components[idx].1 = false;
            }
            ".." => {
                components[idx].1 = false;
                if idx > 0 {
                    components[idx - 1].1 = false;
                }
            }
            _ => {}
        });

    let mut base_components = base.split('/').filter(|c| !c.is_empty());
    let mut components2 = components.into_iter().filter(|c| c.1).map(|c| (c.0, c.2));
    loop {
        match (components2.next(), base_components.next()) {
            (Some(c), Some(b)) => {
                if c.1 != b {
                    return Err(Exception::Route);
                }
            }
            (Some(c), None) => {
                let mut components3 = vec![c];
                components2.for_each(|cc| components3.push(cc));
                return try_match(components3, path);
            }
            (None, None) => return Ok(path.clone()),
            (None, Some(_)) => return Err(Exception::Route),
        }
    }
}
// "下密密麻麻密密麻麻z  zmm 说%2C%20%2C%E5%92%8C%E6%B2%A1"
// can not handle Fs's path like above now...
fn try_match(components: Vec<(&str, String)>, path: &PathBuf) -> Result<PathBuf, Exception> {
    let mut maybe = components
        .as_slice()
        .iter()
        .fold(path.clone(), |mut out, c| {
            out.push(&c.1);
            out
        });

    if maybe.exists() {
        return Ok(maybe);
    }

    (0..components.len()).into_iter().for_each(|_| {
        maybe.pop();
    });

    let mut out = maybe;
    for c in components {
        out.push(&c.1);
        if !out.exists() {
            out.pop();
            out.push(c.0);
            if !out.exists() {
                return Err(Exception::not_found());
            }
        }
    }
    Ok(out)
}
