use url::percent_encoding::percent_decode;
use tokio_core::reactor::Handle;
use hyper::server::{Request};
use hyper::{header, Method};
use futures_cpupool::CpuPool;
use futures::{future};

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
    C: AsRef<Config> + Clone + Send + 'static,
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
    pub fn call(self, req: Request)-> FutureObject {
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
    let components_raw = req_path_dec
        .split('/')
        .filter(|c| !c.is_empty() && c != &".");

    let components =match components_raw.fold(Ok(vec![]), |cs, c| 
        if cs.is_ok() {
            let mut cs = cs.unwrap();
            match (cs.len()>0, c == "..") {
                (_, false)=> {
                    cs.push(c);
                    Ok(cs)
                },
                (true, true)=>{
                    cs.pop();
                    Ok(cs)
                },
                (false, true)=> {
                    Err(cs)
                },
            }
        } else {
          cs
        }
     ) {
         Ok(o)=> o,
         Err(e)=> e,
     };
    debug!("{} -> {:?}", req_path_dec, components);

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
    // router(req_path_dec:&str, base: &str, path: &PathBuf) -> Result<(String, PathBuf), Error>
    fn test(list: Vec<((&str, &str ,&str), Result<(&str, &str), Error>)>) {
        for (idx, (args, res)) in list.into_iter().enumerate() {
            let (req, base, path) = args;
            let res = res.map(|(u, p)|(u.to_string(), PathBuf::from(p)));
            let res2 = router(req, base, &PathBuf::from(path));
            if res.is_ok() && res2.is_ok()&& res.as_ref().unwrap() != res2.as_ref().unwrap() ||  
            ! (res.is_ok() && res2.is_ok()) && !(res.is_err() && res2.is_err())  {
                panic!(format! ("\n{:?} != {:2}\n{:?} <= router(\"{}\", \"{}\", \"{}\")\n",res, idx, res2, req, base, path));
            }
        }
    }
    assert!(Path::new("tests/index").exists());

    test( vec!(
        (("/", "/", "tests"),Ok(("/", "tests"))),
        (("/../", "/", "tests"),Ok(("/", "tests"))),
        (("/../../", "/", "tests"),Ok(("/", "tests"))),
        (("/index", "/", "tests"),Ok(("/index", "tests/index"))),
        (("/index/", "/", "tests"),Ok(("/index/", "tests/index"))),
        (("/index/../", "/", "tests"),Ok(("/", "tests"))),
        // 301 "" -> "/index/.././" by StaticIndex...
        (("/index/../.", "/", "tests"),Ok(("", "tests"))),
        (("/index/.././", "/", "tests"),Ok(("/", "tests"))),
        (("/index/../..", "/", "tests"),Ok(("", "tests"))),
        (("/index/../../", "/", "tests"),Ok(("/", "tests"))),
        (("/index/file", "/", "tests"),Ok(("/index/file", "tests/index/file"))),
        (("/index/file/", "/", "tests"),Ok(("/index/file/", "tests/index/file"))),
        (("/index/file/../", "/", "tests"),Ok(("/index/", "tests/index"))),
        (("/index/file/../../", "/", "tests"),Ok(("/", "tests"))),
        (("/index/file/../..", "/", "tests"),Ok(("", "tests"))),
        (("/index/../file/../../", "/", "tests"),Ok(("/", "tests"))),
        (("/index/../../file/../../", "/", "tests"),Ok(("/", "tests"))),
    ));
}
