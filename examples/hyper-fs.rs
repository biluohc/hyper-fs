#[macro_use]
extern crate log;
extern crate mxo_env_logger;
use mxo_env_logger::*;
extern crate hyper;
extern crate poolite;

use hyper::server::Http;
use poolite::{Builder, Pool};

extern crate hyper_fs;
extern crate num_cpus;
use hyper_fs::{Config, FsPool, StaticFs};

use std::sync::Arc;
use std::env;

fn main() {
    init().expect("Init Log Failed");
    let port = env::args()
        .nth(1)
        .unwrap_or_else(|| "8000".to_owned())
        .parse::<u16>()
        .unwrap();
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    let path = env::args().nth(2).unwrap_or_else(|| "./".to_owned());

    let pool = Pool2(Arc::new(
        Builder::new()
            .max(num_cpus::get() + 1)
            .name("hyper-fs")
            .daemon(None)
            .timeout(None)
            .build()
            .unwrap(),
    ));

    let mut server = Http::new()
        .bind(&addr, move || {
            Ok({
                let config = Config::new().show_index(true).follow_links(true);
                let mut sfs = StaticFs::new("/", &path, pool.clone());
                sfs.set_config(config);
                sfs
            })
        })
        .unwrap();
    server.no_proto();
    println!(
        "Listening on http://{} with 1 thread.",
        server.local_addr().unwrap()
    );
    server.run().unwrap();
}

#[derive(Debug)]
struct Pool2(Arc<Pool>);
impl FsPool for Pool2 {
    fn push<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.0.push(task)
    }
}
impl Clone for Pool2 {
    fn clone(&self) -> Self {
        Pool2(self.0.clone())
    }
}
