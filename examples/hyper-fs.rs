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
use hyper::header::Headers;
use hyper::Error;
use url::percent_encoding::percent_decode;

extern crate hyper_fs;
use hyper_fs::{error_handler, Config, HyperFutureObject, StaticFs};

use std::path::PathBuf;
use std::time::Instant;
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
    let path = env::args().nth(2).unwrap_or_else(|| ".".to_owned());

    let pool = Builder::new().pool_size(3).name_prefix("hyper-fs").create();
    let config = Config::new()
        .cache_secs(60)
        .follow_links(true)
        .show_index(true); // .chunk_size(8196)

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let listener = TcpListener::bind(&addr, &handle).unwrap();
    let fs_server = Rc::new(FileServer::new(handle.clone(), pool, path, config));

    let http = Http::new();
    let server = listener.incoming().for_each(|(socket, addr)| {
        http.bind_connection(&handle, socket, addr, fs_server.clone());
        Ok(())
    });
    println!("Listening on http://{} with 1 thread.", addr);
    core.run(server).unwrap();
}

struct FileServer {
    handle: Handle,
    path: PathBuf,
    pool: CpuPool,
    headers_index: Option<Headers>,
    config: Arc<Config>,
}
impl FileServer {
    fn new<P: Into<PathBuf>>(handle: Handle, pool: CpuPool, path: P, config: Config) -> Self {
        Self {
            handle: handle,
            path: path.into(),
            pool: pool,
            config: Arc::new(config),
            headers_index: Some({
                let mut tmp = Headers::new();
                tmp.set_raw("Content-Type", "text/html; charset=utf-8");
                tmp
            }),
        }
    }
}

impl Service for FileServer {
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = HyperFutureObject;
    fn call(&self, req: Request) -> Self::Future {
        let timer = Instant::now();
        let mut fs = StaticFs::new(
            self.handle.clone(),
            self.pool.clone(),
            "/",
            &self.path,
            self.config.clone(),
        );
        // add `Content-Type` for index, `StaticFs` alrendy add `Content-Type` by `content_type_maker`(mime_guess crate) default.
        // use `static_fs` or `StaticFile` or `static_file` directly if need to use custom `Content-Type` for file.
        *fs.headers_index_mut() = self.headers_index.clone();

        let fs = fs.call(req);

        Box::new(fs.or_else(error_handler).map(move |res_req| {
            // remote addr
            let addr = res_req
                .1
                .remote_addr()
                .expect("Request.remote_addr() is None");
            // cost time
            let time = timer.elapsed();
            // let ms = (time.as_secs()* 1000) as u32 + time.subsec_millis();
            let ms = (time.as_secs() * 1000) as u32 + time.subsec_nanos() / 1_000_000;

            // log
            println!(
                "[{}:{} {:3}ms] {} {} {}",
                addr.ip(),
                addr.port(),
                ms,
                res_req.0.status().as_u16(),
                res_req.1.method(),
                percent_decode(res_req.1.path().as_bytes())
                    .decode_utf8()
                    .unwrap()
                    .into_owned()
                    .to_owned()
            );
            // extract response
            res_req.0
        }))
    }
}
