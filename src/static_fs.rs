use hyper::server::{Request, Response, Service};
use hyper::{Error, Method};
use futures_cpupool::CpuPool;
use tokio_core::reactor::Handle;
use url::percent_encoding::percent_decode;

use super::{Exception, ExceptionHandler, ExceptionHandlerService};
use super::{StaticFile, StaticIndex};
use super::FutureResponse;
use super::Config;

use std::path::PathBuf;

/// Static File System
// Todu: full test...
pub struct StaticFs<C, EH = ExceptionHandler> {
    url: String,   // http's base path
    path: PathBuf, // Fs's base path
    handle: Handle,
    pool: CpuPool,
    config: C,
    handler: EH,
}

impl<C> StaticFs<C, ExceptionHandler>
where
    C: AsRef<Config> + Clone,
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
    C: AsRef<Config> + Clone,
    EH: ExceptionHandlerService + Clone,
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
        }
    }
    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }
}

impl<C, EH> Service for StaticFs<C, EH>
where
    C: AsRef<Config> + Clone,
    EH: ExceptionHandlerService + Clone,
{
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureResponse;
    fn call(&self, req: Request) -> Self::Future {
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return self.handler.call(Exception::Method, req),
        }
        let req_path = percent_decode(req.path().as_bytes())
            .decode_utf8()
            .unwrap()
            .into_owned(); // path() is str?, so safe?
        debug!(
            "\nurl : {:?}\npath: {:?}\nreqRaw: {:?}\nreqDec: {:?}",
            self.url,
            self.path,
            req.path(),
            req_path,
        );
        let fspath = match route(&req_path, &self.url, &self.path) {
            Some(p) => p,
            None => {
                return self.handler.call(Exception::Route, req);
            }
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
                        StaticFile::with_handler(
                        self.handle.clone(),
                        self.pool.clone(),
                        fspath,
                        config,
                        self.handler.clone(),
                    ).call(req)         
                } else if md.is_dir() {
                    StaticIndex::with_handler(fspath, config, self.handler.clone()).call(req)
                } else {
                    self.handler.call(Exception::Typo, req)
                }   
            }
            Err(e) => self.handler.call(e, req),
        }
    }
}

fn route(p: &str, base: &str, path: &PathBuf) -> Option<PathBuf> {
    let mut components = p.split('/')
        .filter(|c| !c.is_empty())
        .map(|c| (c, true))
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
    let mut components2 = components.iter().filter(|c| c.1).map(|c| c.0);
    loop {
        match (components2.next(), base_components.next()) {
            (Some(c), Some(b)) => {
                if c != b {
                    return None;
                }
            }
            (Some(c), None) => {
                let mut out = path.clone();
                out.push(c);
                components2.for_each(|cc| out.push(cc));
                return Some(out);
            }
            (None, None) => return Some(path.clone()),
            (None, Some(_)) => return None,
        }
    }
}
