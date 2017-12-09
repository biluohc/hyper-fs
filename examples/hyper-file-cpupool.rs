#[macro_use]
extern crate log;
extern crate mxo_env_logger;
use mxo_env_logger::*;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;

use futures::future::Executor;
use futures_cpupool::{Builder, CpuPool};
use futures::{future, Async, Future, Poll, Sink, Stream};
 use futures::future::FutureResult;
use hyper::server::Http;

extern crate hyper_fs;
extern crate num_cpus;
use hyper_fs::{StaticFile, FsPool};

use std::env;

fn main() {
    init().expect("Init Log Failed");
    let port = env::args()
        .nth(1)
        .unwrap_or_else(|| "8000".to_owned())
        .parse::<u16>()
        .unwrap();
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    let file = env::args().nth(2).unwrap_or_else(|| "fn.jpg".to_owned());

    let pool = Pool2(
        Builder::new()
            .pool_size(num_cpus::get() + 1)
            .name_prefix("hyper-fs")
            .create(),
    );

    let mut server = Http::new()
        .bind(&addr, move || Ok(StaticFile::new(pool.clone(), &file)))
        .unwrap();
    server.no_proto();
    println!(
        "Listening on http://{} with 1 thread.",
        server.local_addr().unwrap()
    );
    server.run().unwrap();
}

#[derive(Debug)]
struct Pool2(CpuPool);
impl Clone for Pool2 {
    fn clone(&self) -> Self {
        Pool2(self.0.clone())
    }
}
impl FsPool for Pool2 {
    fn push<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        fn fuck<F>(task: F) -> FutureResult<(), ()>
        where
            F: FnOnce() + Send + 'static,
        {
            task();
            future::ok(())
        }
        self.0.execute(fuck(task)).unwrap();
    }
}
