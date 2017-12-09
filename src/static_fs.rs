use futures::future::FutureResult;
use hyper::server::{Request, Response, Service};
use hyper::{Error, Method};

use super::{ExceptionCatcher, ExceptionHandler};
use super::StaticIndex;
use super::StaticFile;
use super::FsPool;
use super::Config;

use std::io::{self, BufReader, Read};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use std::{mem, time};
use std::borrow::BorrowMut;

/// Static FileSystem
// Todu: full test...
pub struct StaticFs<FP: FsPool, EH = ExceptionHandler> {
    url: PathBuf,  // http's base path
    path: PathBuf, // Fs's base path
    pool: FP,
    handler: EH,
    config: Box<Config>,
}

impl<FP: FsPool> StaticFs<FP, ExceptionHandler> {
    pub fn new<P>(url: P, path: P, pool: FP) -> Self
    where
        P: Into<PathBuf>,
    {
        Self::with_handler(url, path, pool, ExceptionHandler::default())
    }
}

impl<FP: FsPool, EH: ExceptionCatcher> StaticFs<FP, EH> {
    pub fn with_handler<P>(url: P, path: P, pool: FP, handler: EH) -> Self
    where
        P: Into<PathBuf>,
    {
        Self {
            url: url.into(),
            path: path.into(),
            pool: pool,
            handler: handler,
            config: Box::new(Config::new()),
        }
    }
    pub fn set_config(&mut self, config: Config) {
        *self.config.borrow_mut() = config;
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
}

impl<FP, EH: ExceptionCatcher> Service for StaticFs<FP, EH>
where
    FP: FsPool + 'static,
    EH: Service<
        Request = Request,
        Response = Response,
        Error = Error,
        Future = FutureResult<Response, Error>,
    >,
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
        let req_path = req.path().to_owned();
        let fspath = match Path::new(&req_path).strip_prefix(&self.url) {
            Ok(p) => {
                debug_assert!(!p.has_root());
                let mut tmp = self.path.clone();
                tmp.push(p);
                tmp
            }
            Err(_) => {
                return self.handler.call(req);
            }
        };
        let metadata = if self.config().get_follow_links() {
            fspath.metadata()
        } else {
            fspath.symlink_metadata()
        };
        match metadata {
            Ok(md) => {
                let config = *self.config.clone();
                if md.is_dir() {
                    let mut index_service = StaticIndex::new(fspath);
                    index_service.set_config(config);
                    index_service.call(req)
                } else {
                    let mut file_service = StaticFile::new(self.pool.clone(), fspath);
                    file_service.set_config(config);
                    file_service.call(req)
                }
            }
            Err(e) => {
                self.handler.catch(e);
                self.handler.call(req)
            }
        }
    }
}
