pub use hyper::server::{Request, Response, Service};
pub use hyper::{header, Error, Method};
pub use futures_cpupool::CpuPool;
pub use tokio_core::reactor::Handle;
pub use url::percent_encoding::percent_decode;

pub use super::{Exception, ExceptionHandlerService, ExceptionHandlerServiceAsync};
pub use super::FutureObject;
pub use super::Config;

#[cfg(feature = "default")]
use super::content_type_maker;
#[cfg(feature = "default")]
use super::ExceptionHandler;
#[cfg(feature = "default")]
use super::StaticFile;
#[cfg(feature = "default")]
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
    ($typo_fs: ident, $typo_file: ident, $fn_content_type_maker:ident, $typo_index: ident, $typo_exception_handler: ident) => {
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
        let req_path_dec = percent_decode(req.path().as_bytes())
        .decode_utf8()
        .unwrap()
        .into_owned()
        .to_owned();
        debug!("{}", req_path_dec);

        let res_after_router = router(&req_path_dec, &self.url, &self.path);
        debug!(
            "\nurl/path: {:?} -> {:?}\nreqRaw: {:?}\nreqDec_afterRouter: {:?}",
            self.url,
            self.path,
            req.path(),
            res_after_router,
        );
        let (req_path,fspath) = match res_after_router {
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
                    file_server.headers_maker($fn_content_type_maker);
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
    };
}

pub fn router(req_path_dec: &str, base: &str, path: &PathBuf) -> Result<(String, PathBuf), Exception> {
    let mut components = req_path_dec
        .split('/')
        .filter(|c| !c.is_empty() && c != &".")
        .collect::<Vec<_>>();

    let parent_count = (0..components.len())
        .into_iter()
        .rev()
        .fold(0, |count, idx| match (idx < components.len(), idx > 0) {
            (true, true) => match (components[idx] == "..", components[idx - 1] == "..") {
                (false, _) => count,
                (true, false) => {
                    components.remove(idx);
                    components.remove(idx - 1);
                    count
                }
                (true, true) => {
                    components.remove(idx);
                    count + 1
                }
            },
            (false, _) => count,
            (true, false) => {
                if count >= components.len() {
                    components.clear();
                    count
                } else {
                    let new_len = components.len() - count;
                    components.truncate(new_len);
                    components.first().map(|f| *f == "..").map(|b| {
                        if b {
                            components.clear()
                        }
                    });
                    count
                }
            }
        });
    debug!("{}: {:?}", parent_count, components);

    let req_path = || {
        let mut tmp = components
            .iter()
            .fold(String::with_capacity(req_path_dec.len()), |mut p, c| {
                p.push('/');
                p.push_str(c);
                p
            });
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
                    return Ok((req_path(), out));
                } else {
                    return Err(Exception::not_found());
                }
            }
            (None, None) => return Ok((req_path(), path.clone())),
            (None, Some(_)) => return Err(Exception::Route),
        }
    }
}

#[test]
fn router_test() {
    use std::path::Path;
    assert!(Path::new("tests/index").exists());
    // router(req_path_dec:&str, base: &str, path: &PathBuf) -> Result<(String, PathBuf), Exception>
    assert_eq!(
        router("/", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/../", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/../../", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/index", "/", &PathBuf::from("tests")).unwrap(),
        ("/index".to_owned(), PathBuf::from("tests/index"))
    );
    assert_eq!(
        router("/index/", "/", &PathBuf::from("tests")).unwrap(),
        ("/index/".to_owned(), PathBuf::from("tests/index"))
    );
    assert_eq!(
        router("/index/../", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
    // 301 "" -> "/index/.././" by StaticIndex...
    assert_eq!(
        router("/index/../.", "/", &PathBuf::from("tests")).unwrap(),
        ("".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/index/.././", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/index/../..", "/", &PathBuf::from("tests")).unwrap(),
        ("".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/index/../../", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/index/file", "/", &PathBuf::from("tests")).unwrap(),
        ("/index/file".to_owned(), PathBuf::from("tests/index/file"))
    );
    assert_eq!(
        router("/index/file/", "/", &PathBuf::from("tests")).unwrap(),
        ("/index/file/".to_owned(), PathBuf::from("tests/index/file"))
    );
    assert_eq!(
        router("/index/file/../", "/", &PathBuf::from("tests")).unwrap(),
        ("/index/".to_owned(), PathBuf::from("tests/index"))
    );
    assert_eq!(
        router("/index/file/../../", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/index/file/../..", "/", &PathBuf::from("tests")).unwrap(),
        ("".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/index/../file/../../", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
    assert_eq!(
        router("/index/../../file/../../", "/", &PathBuf::from("tests")).unwrap(),
        ("/".to_owned(), PathBuf::from("tests"))
    );
}

#[cfg(feature = "default")]
static_fs!(
    StaticFs,
    StaticFile,
    content_type_maker,
    StaticIndex,
    ExceptionHandler
);
