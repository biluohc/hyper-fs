use futures::future::FutureResult;
use hyper::server::{Request, Response, Service};
use hyper::{Error, Method};
use futures_cpupool::CpuPool;
use tokio_core::reactor::Handle;
use url::percent_encoding::percent_decode;

use super::{Exception, ExceptionHandler, ExceptionHandlerService};
use super::StaticIndex;
use super::StaticFile;
use super::Config;

use std::path::{Path, PathBuf};

/// Static File System
// Todu: full test...
pub struct StaticFs<C, EH = ExceptionHandler> {
    url: PathBuf,  // http's base path
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
    pub fn new<P0, P1>(handle: Handle, pool: CpuPool, url: P0, path: P1, config: C) -> Self
    where
        P0: Into<PathBuf>,
        P1: Into<PathBuf>,
    {
        Self::with_handler(handle, pool, url, path, config, ExceptionHandler::default())
    }
}

impl<C, EH> StaticFs<C, EH>
where
    C: AsRef<Config> + Clone,
    EH: ExceptionHandlerService + Clone,
{
    pub fn with_handler<P0, P1>(handle: Handle, pool: CpuPool, url: P0, path: P1, config: C, handler: EH) -> Self
    where
        P0: Into<PathBuf>,
        P1: Into<PathBuf>,
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
    type Future = FutureResult<Response, Error>;
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
            "\nurl : {:?}\npath: {:?}\nreqRaw: {:?}\nreqDec: {:?}\nmatch: {:?}",
            self.url,
            self.path,
            req.path(),
            req_path,
            Path::new(&req_path).strip_prefix(&self.url)
        );
        let fspath = match Path::new(&req_path).strip_prefix(&self.url) {
            Ok(p) => {
                debug_assert!(!p.has_root());
                let mut tmp = self.path.clone();
                tmp.push(p);
                tmp
            }
            Err(_) => {
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
                if md.is_dir() {
                    StaticIndex::with_handler(fspath, config, self.handler.clone()).call(req)
                } else {
                    StaticFile::with_handler(
                        self.handle.clone(),
                        self.pool.clone(),
                        fspath,
                        config,
                        self.handler.clone(),
                    ).call(req)
                }
            }
            Err(e) => self.handler.call(e, req),
        }
    }
}
