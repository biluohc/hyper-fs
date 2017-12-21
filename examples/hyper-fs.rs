// #[macro_use]
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
extern crate log;
extern crate mxo_env_logger;
extern crate num_cpus;
extern crate tokio_core;
extern crate url;

use mxo_env_logger::*;
use futures_cpupool::Builder;
use tokio_core::reactor::Core;
use tokio_core::net::TcpListener;
use hyper::server::Http;
use futures::Stream;

extern crate hyper_fs;
use hyper_fs::{Config, StaticFs};

use std::sync::Arc;
use std::rc::Rc;
use std::env;

fn main() {
    init().expect("Init Log Failed");
    let port = env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(8000u16);
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    let file = env::args().nth(2).unwrap_or_else(|| ".".to_owned());

    let pool = Builder::new().pool_size(3).name_prefix("hyper-fs").create();
    let config = Arc::new(
        Config::new()
            .cache_secs(60)
            .follow_links(true)
            .show_index(true), // .chunk_size(8196)
    );

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let listener = TcpListener::bind(&addr, &handle).unwrap();
    let fs = StaticFs::new(handle.clone(), pool, "/", &file, config);
    let fs = Rc::new(fs);

    let http = Http::new();
    let server = listener.incoming().for_each(|(socket, addr)| {
        http.bind_connection(&handle, socket, addr, fs.clone());
        Ok(())
    });
    println!("Listening on http://{} with 1 thread.", addr);
    core.run(server).unwrap();
}
