extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate mxo_env_logger;
extern crate num_cpus;
extern crate tokio_core;
extern crate url;

use mxo_env_logger::*;
use futures_cpupool::{Builder, CpuPool};
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::TcpListener;
use futures::{future, Future, Stream};
use hyper::server::{Http, Request, Response, Service};
use hyper::{header, Error, StatusCode};

extern crate hyper_fs;
use hyper_fs::{error_handler, Config, FutureObject, HyperFutureObject, StaticFs};

use std::path::PathBuf;
use std::sync::Arc;
use std::rc::Rc;
use std::env;
use std::fs;

fn main() {
    init().expect("Init Log Failed");
    let port = env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(8000u16);
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();

    let pool = Builder::new().pool_size(3).name_prefix("hyper-fs").create();
    let config = Config::new()
        .cache_secs(60)
        .follow_links(true)
        .show_index(true);

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let listener = TcpListener::bind(&addr, &handle).unwrap();
    let doc = Doge::new(handle.clone(), pool, config);

    let http = Http::new();
    let server = listener.incoming().for_each(|(socket, addr)| {
        http.bind_connection(&handle, socket, addr, doc.clone());
        Ok(())
    });
    println!("Listening on http://{} with 1 thread.", addr);
    core.run(server).unwrap();
}

pub struct DogeInner {
    doc: Option<PathBuf>,  // target/doc
    rust: Option<PathBuf>, // $HOME/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/share/doc/rust/html
    path: PathBuf,         // default: ./
    pool: CpuPool,
    handle: Handle,
    config: Arc<Config>,
}

#[derive(Clone)]
pub struct Doge {
    inner: Rc<DogeInner>,
}

fn rust() -> Option<PathBuf> {
    let mut r = PathBuf::from(env::var("HOME").unwrap());
    r.push(".rustup/toolchains/");
    if let Ok(entrys) = fs::read_dir(&r) {
        let entrys = entrys.filter_map(|e| e.ok()).collect::<Vec<_>>();
        for entry in entrys {
            let mut path = entry.path();
            path.push("share/doc/rust/html");
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

impl Doge {
    fn new(handle: Handle, pool: CpuPool, config: Config) -> Self {
        let doc = PathBuf::from("target/doc");
        let doc = if doc.as_path().is_dir() {
            Some(doc)
        } else {
            None
        };
        let path = PathBuf::from(env::args().nth(2).unwrap_or_else(|| "./".to_owned()));
        let inner = DogeInner {
            doc: doc,
            rust: rust(),
            path: path,
            pool: pool,
            handle: handle,
            config: Arc::new(config),
        };
        Doge {
            inner: Rc::new(inner),
        }
    }
    fn call_inner(&self, req: Request) -> FutureObject {
        let path = req.path().to_owned();
        // /doc
        if self.inner.doc.is_some() && (path.starts_with("/doc/") || path.as_str() == "/doc") {
            StaticFs::new(
                self.inner.handle.clone(),
                self.inner.pool.clone(),
                "/doc/",
                self.inner.doc.as_ref().unwrap(),
                self.inner.config.clone(),
            ).call(req)

        // /rust
        } else if self.inner.rust.is_some() && path.as_str() == "/rust" {
            Box::new(future::ok((
                Response::new()
                    .with_status(StatusCode::MovedPermanently)
                    .with_header(header::Location::new("/rust/index.html")),
                req,
            )))
        } else if self.inner.rust.is_some() && path.starts_with("/rust/") {
            StaticFs::new(
                self.inner.handle.clone(),
                self.inner.pool.clone(),
                "/rust/",
                self.inner.rust.as_ref().unwrap(),
                self.inner.config.clone(),
            ).call(req)
        }
        // path
        else {
            StaticFs::new(
                self.inner.handle.clone(),
                self.inner.pool.clone(),
                "/",
                self.inner.path.clone(),
                self.inner.config.clone(),
            ).call(req)
        }
    }
}

impl Service for Doge {
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = HyperFutureObject;

    fn call(&self, req: Request) -> Self::Future {
        Box::new(
            self.call_inner(req)
                .or_else(error_handler)
                .map(|res_req| res_req.0),
        )
    }
}
