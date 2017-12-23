pub use hyper::server::{Request, Response, Service};
pub use hyper::{header, Error, Method};
pub use futures_cpupool::CpuPool;
pub use tokio_core::reactor::Handle;
pub use url::percent_encoding::percent_decode;

pub use super::{Exception, ExceptionHandlerService, ExceptionHandlerServiceAsync};
pub use super::FutureObject;
pub use super::Config;

use super::ExceptionHandler;
use super::StaticFile;
use super::StaticIndex;

pub use std::path::PathBuf;

/**
create a `StaticFs` by owner types.

```rs
#[macro_use]
extern crate hyper_fs;

pub mod fs_server {
    use hyper_fs::static_fs::*;
    use hyper_fs::{StaticFile, StaticIndex,ExceptionHandler};
    static_fs!(StaticFs2,StaticFile, StaticIndex, ExceptionHandler);
}
```
*/
#[macro_export]
macro_rules! static_fs {
    ($typo_fs: ident, $typo_file: ident, $typo_index: ident, $typo_exception_handler: ident) => {
/// Static File System
// Todu: full test...
pub struct $typo_fs<C> {
    url: String,   // http's base path
    path: PathBuf, // Fs's base path
    handle: Handle,
    pool: CpuPool,
    headers_file: Option<header::Headers>,
    headers_index: Option<header::Headers>,
    config: C,
}

impl<C> $typo_fs<C>
where
    C: AsRef<Config> + Clone + Send,
{
    pub fn new<U, P>(handle: Handle, pool: CpuPool, url: U, path: P, config: C) -> Self
    where
        U: Into<String>,
        P: Into<PathBuf>,
    {
        Self {
            url: url.into(),
            path: path.into(),
            handle: handle,
            pool: pool,
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

impl<C> Service for $typo_fs<C>
where
    C: AsRef<Config> + Clone + Send + 'static,
{
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureObject;
    fn call(&self, req: Request) -> Self::Future {
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return $typo_exception_handler::call_async(Exception::Method, req),
        }
        debug!(
            "\nurl : {:?}\npath: {:?}\nreqRaw: {:?}\nreqDec: {:?}",
            self.url,
            self.path,
            req.path(),
            route(req.path(), &self.url, &self.path),
        );
        let (req_path,fspath) = match route(req.path(), &self.url, &self.path) {
            Ok(p) => p,
            Err(e) => return $typo_exception_handler::call_async(e, req),
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
                    let mut file_server = $typo_file::new(
                        self.handle.clone(),
                        self.pool.clone(),
                        fspath,
                        config.as_ref().clone(),
                    );
                    if self.headers_file.is_some() {
                        *file_server.headers_mut() = self.headers_file.clone();
                    }
                    file_server.call(&self.pool, req)
                } else if md.is_dir() {
                    let mut index_server = $typo_index::new(req_path, fspath, config.clone());
                    if self.headers_index.is_some() {
                        *index_server.headers_mut() = self.headers_index.clone();
                    }
                    index_server.call(&self.pool, req)
                } else {
                    $typo_exception_handler::call_async(Exception::Typo, req)
                }
            }
            Err(e) => $typo_exception_handler::call_async(e, req),
        }
    }
}

fn route(req_path: &str, base: &str, path: &PathBuf) -> Result<(String, PathBuf), Exception> {
    let req_path_dec = percent_decode(req_path.as_bytes())
        .decode_utf8()
        .unwrap()
        .into_owned()
        .to_owned();
    debug!("{}", req_path_dec);
    let mut components = req_path_dec.split('/')
        .filter(|c| !c.is_empty() && c != &".")
        .collect::<Vec<_>>();
    (0..components.len())
        .into_iter()
        .rev()
        .for_each(|idx| if idx<components.len()&& components[idx] == ".."  {
                components.remove(idx);
                if idx > 0 {
                    components.remove(idx-1);
                }
        });

    let req_path =|| {
        let mut tmp = components.iter().fold(String::with_capacity(req_path_dec.len()),| mut p, c| {
                p.push('/');
                p.push_str(c);
                p
            }
        );
        if req_path_dec.ends_with('/') {
            tmp.push('/');
        }
        tmp
    };
    let mut base_components = base.split('/').filter(|c| !c.is_empty());
    let mut components2 = components.iter();
    loop {
        match (components2.next(), base_components.next()) {
            (Some(c), Some(b)) => {
                if c != &b {
                    return Err(Exception::Route);
                }
            }
            (Some(c), None) => {
                let mut out = path.clone();
                out.push(c);
                components2.for_each(|cc| out.push(cc));
                if out.exists() {
                    return Ok( (req_path(),out));
                } else {
                    return Err(Exception::not_found());
                }
            }
            (None, None) => {
                return Ok( (req_path(), path.clone())) },
            (None, Some(_)) => return Err(Exception::Route),
        }
    }
}
    };
}

static_fs!(StaticFs, StaticFile, StaticIndex, ExceptionHandler);
