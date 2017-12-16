use futures::future::FutureResult;
use hyper::server::{Request, Response, Service};
use hyper::{Error, Method};
use futures_cpupool::CpuPool;
use tokio_core::reactor::Handle;

use super::{ExceptionCatcher, ExceptionHandler};
use super::StaticIndex;
use super::StaticFile;
use super::Config;

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Static FileSystem
// Todu: full test...
pub struct StaticFs<EH = ExceptionHandler> {
    url: PathBuf,  // http's base path
    path: PathBuf, // Fs's base path
    handle: Handle,
    pool: CpuPool,
    config: Arc<Config>,
    handler: EH,
}

impl StaticFs<ExceptionHandler> {
    pub fn new<P>(url: P, path: P, handle: Handle, pool: CpuPool, config: Arc<Config>) -> Self
    where
        P: Into<PathBuf>,
    {
        Self::with_handler(url, path, handle, pool, config, ExceptionHandler::default())
    }
}

impl<EH: ExceptionCatcher> StaticFs<EH> {
    pub fn with_handler<P>(url: P, path: P, handle: Handle, pool: CpuPool, config: Arc<Config>, handler: EH) -> Self
    where
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
        &self.config
    }
}

impl<EH: ExceptionCatcher> Service for StaticFs<EH>
where
    EH: Service<Request = Request, Response = Response, Error = Error, Future = FutureResult<Response, Error>>,
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
                let config = self.config.clone();
                if md.is_dir() {
                    let index_service = StaticIndex::new(fspath, config);
                    index_service.call(req)
                } else {
                    let file_service = StaticFile::new(self.handle.clone(), self.pool.clone(), fspath, config);
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
