// #[macro_use]
// extern crate log;
extern crate mxo_env_logger;
use mxo_env_logger::*;
extern crate hyper;
use hyper::server::Http;

extern crate hyper_fs;
use hyper_fs::{Config, StaticIndex};

use std::env;

fn main() {
    init().expect("Init Log Failed");
    let port = env::args()
        .nth(1)
        .unwrap_or_else(|| "8000".to_owned())
        .parse::<u16>()
        .unwrap();
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    let index = env::args().nth(2).unwrap_or_else(|| ".".to_owned());

    let mut server = Http::new()
        .bind(&addr, move || {
            Ok({
                let config = Config::new().show_index(true).follow_links(true);
                let mut index_service = StaticIndex::new(&index);
                index_service.set_config(config);
                index_service
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
