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
use futures::{Future, Stream};
use hyper::server::{Http, Request, Response, Service};
use hyper::Error;

extern crate hyper_fs;
use hyper_fs::{error_handler, Config, HyperFutureObject, StaticFile};

use std::path::PathBuf;
use std::sync::Arc;
use std::rc::Rc;
use std::env;

fn main() {
    init().expect("Init Log Failed");
    let port = env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u16>().ok())
        .unwrap_or(8000);
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    let index = env::args().nth(2).unwrap_or_else(|| ".".to_owned());

    let pool = Builder::new().pool_size(3).name_prefix("hyper-fs").create();
    let config = Config::new()
        .cache_secs(60)
        .follow_links(true)
        .show_index(true); // .chunk_size(8196)

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let listener = TcpListener::bind(&addr, &handle).unwrap();
    let file_server = Rc::new(FileServer::new(handle.clone(), pool, index, config));

    let http = Http::new();
    let server = listener.incoming().for_each(|(socket, addr)| {
        http.bind_connection(&handle, socket, addr, file_server.clone());
        Ok(())
    });
    println!("Listening on http://{} with 1 thread.", addr);
    core.run(server).unwrap();
}

struct FileServer {
    handle: Handle,
    file: PathBuf,
    pool: CpuPool,
    config: Arc<Config>,
}
impl FileServer {
    fn new<P: Into<PathBuf>>(handle: Handle, pool: CpuPool, file: P, config: Config) -> Self {
        Self {
            handle: handle,
            file: file.into(),
            pool: pool,
            config: Arc::new(config),
        }
    }
}

impl Service for FileServer {
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = HyperFutureObject;
    fn call(&self, req: Request) -> Self::Future {
        Box::new(
            StaticFile::new(
                self.handle.clone(),
                self.pool.clone(),
                &self.file,
                self.config.clone(),
            ).call(&self.pool, req)
                .or_else(error_handler)
                .map(|res_req| res_req.0),
        )
    }
}
