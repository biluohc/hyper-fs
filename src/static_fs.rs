use hyper::server::{Request, Response, Service};
use hyper::{header, Method};
use url::percent_encoding::percent_decode;
use tokio_core::reactor::Handle;
use futures_cpupool::CpuPool;
use futures::future;

use super::{Config, Error, FutureObject};
use super::{StaticFile, StaticIndex};

#[cfg(feature = "default")]
use super::content_type_maker;

use std::path::PathBuf;

/// Static File System
// Todu: full test...
pub struct StaticFs<C> {
    url: String,   // http's base path
    path: PathBuf, // Fs's base path
    handle: Handle,
    pool: CpuPool,
    headers_file: Option<header::Headers>,
    headers_index: Option<header::Headers>,
    config: C,
}

impl<C> StaticFs<C>
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

impl<C> Service for StaticFs<C>
where
    C: AsRef<Config> + Clone + Send + 'static,
{
    type Request = Request;
    type Response = Response;
    type Error = (Error, Request);
    type Future = FutureObject;
    fn call(&self, req: Request) -> Self::Future {
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return Box::new(future::err((Error::Method, req))),
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
        let (req_path, fspath) = match res_after_router {
            Ok(p) => p,
            Err(e) => return Box::new(future::err((e, req))),
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
                    let mut file_server = StaticFile::new(
                        self.handle.clone(),
                        self.pool.clone(),
                        fspath,
                        config.as_ref().clone(),
                    );
                    if self.headers_file.is_some() {
                        *file_server.headers_mut() = self.headers_file.clone();
                    }
                    file_server.headers_maker(content_type_maker);
                    file_server.call(&self.pool, req)
                } else if md.is_dir() {
                    let mut index_server = StaticIndex::new(req_path, fspath, config.clone());
                    if self.headers_index.is_some() {
                        *index_server.headers_mut() = self.headers_index.clone();
                    }
                    index_server.call(&self.pool, req)
                } else {
                    Box::new(future::err((Error::Typo, req)))
                }
            }
            Err(e) => Box::new(future::err((e.into(), req))),
        }
    }
}

pub fn router(req_path_dec: &str, base: &str, path: &PathBuf) -> Result<(String, PathBuf), Error> {
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
                    return Err(Error::Route);
                }
            }
            (Some(c), None) => {
                let mut out = path.clone();
                out.push(c);
                components2.for_each(|cc| out.push(cc));
                if out.exists() {
                    return Ok((req_path(), out));
                } else {
                    return Err(Error::not_found());
                }
            }
            (None, None) => return Ok((req_path(), path.clone())),
            (None, Some(_)) => return Err(Error::Route),
        }
    }
}

#[test]
fn router_test() {
    use std::path::Path;
    assert!(Path::new("tests/index").exists());
    // router(req_path_dec:&str, base: &str, path: &PathBuf) -> Result<(String, PathBuf), Error>
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
