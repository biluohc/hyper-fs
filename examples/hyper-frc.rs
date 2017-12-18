#[macro_use]
extern crate log;
extern crate mxo_env_logger;
use mxo_env_logger::*;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
extern crate tokio_core;
extern crate url;

use futures_cpupool::Builder;
use tokio_core::reactor::{Core};
use tokio_core::net::TcpListener;
use futures::{Stream};
use hyper::server::Http;

extern crate hyper_fs;
extern crate num_cpus;
use hyper_fs::{StaticFile,Config};

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
    let file = env::args()
        .nth(2)
        .unwrap_or_else(|| "fn.jpg".to_owned());

    let pool = Builder::new()
        .pool_size(2)
        .name_prefix("hyper-fs")
        .create();

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let listener = TcpListener::bind(&addr, &handle).unwrap();
    let file = Rc::new(StaticFile::new(handle.clone(), pool, file, Config::new().cache_secs(0)));

    let http = Http::new();
    let server = listener.incoming().for_each(|(socket, addr)| {
        http.bind_connection(&handle, socket, addr, file.clone());
        Ok(())
    });
    println!("Listening on http://{} with 1 thread.", addr);
    core.run(server).unwrap();
}