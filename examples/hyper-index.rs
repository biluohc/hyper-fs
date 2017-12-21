// #[macro_use]
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
extern crate log;
extern crate mxo_env_logger;
extern crate tokio_core;
extern crate url;
extern crate num_cpus;

use mxo_env_logger::*;
use futures_cpupool::Builder;
use tokio_core::reactor::Core;
use tokio_core::net::TcpListener;
use hyper::server::Http;
use futures::Stream;
use futures_cpupool::CpuPool;
use hyper::server::{Request, Response, Service};
use hyper::{Error};

extern crate hyper_fs;
use hyper_fs::{Config,FutureObject, StaticIndex};

use std::path::PathBuf;
use std::sync::Arc;
use std::rc::Rc;
use std::env;

fn main() {
    init().expect("Init Log Failed");
    let port = env::args()
        .nth(1)
        .and_then(|arg|
        arg.parse::<u16>().ok())      
        .unwrap_or(8000);
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    let index = env::args().nth(2).unwrap_or_else(|| ".".to_owned());

    let pool = Builder::new().pool_size(3).name_prefix("hyper-fs").create();
    let config = Config::new().cache_secs(60).follow_links(true).show_index(true); // .chunk_size(8196)
            
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let listener = TcpListener::bind(&addr, &handle).unwrap();
    let index_server =Rc::new(IndexServer::new(pool, index, config));

    let http = Http::new();
    let server = listener.incoming().for_each(|(socket, addr)| {
        http.bind_connection(&handle, socket, addr, index_server.clone());
        Ok(())
    });
    println!("Listening on http://{} with 1 thread.", addr);
    core.run(server).unwrap();
}

struct IndexServer {
    path: PathBuf,
    pool: CpuPool,
    config: Arc<Config>,
}
impl IndexServer {
    fn new<P:Into<PathBuf>>(pool: CpuPool, path:P, config: Config)->Self {
        Self {
            path: path.into(),
            pool:pool,
            config: Arc::new(config)
        }
    }
}

impl Service for IndexServer
{
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureObject;
    fn call(&self, req: Request) -> Self::Future {
        StaticIndex::new(&self.path, self.config.clone()).call(&self.pool, req)
    }
}