#[macro_use]
extern crate log;
extern crate mxo_env_logger;
use mxo_env_logger::*;
extern crate hyper;

use hyper::server::Http;

extern crate hyper_fs;
extern crate num_cpus;
use hyper_fs::{Builder,  StaticFile};

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
    let path = env::args().nth(2).unwrap_or_else(|| "fn.jpg".to_owned());

    let pool = Arc::new(
        // Builder::new().pool_size(1).name_prefix("hyper-fs-").build().unwrap()
        Builder::new().pool_size(num_cpus::get()+1).name_prefix("hyper-fs-").build().unwrap()
    );

    let mut server = Http::new()
        .bind(&addr, move || {
            Ok(
                StaticFile::new(&pool,  &path)
            )
        })
        .unwrap();
    server.no_proto();
    println!(
        "Listening on http://{} with 1 thread.",
        server.local_addr().unwrap()
    );
    server.run().unwrap();
}