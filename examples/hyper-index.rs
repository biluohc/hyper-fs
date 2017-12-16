// #[macro_use]
// extern crate log;
extern crate mxo_env_logger;
use mxo_env_logger::*;
extern crate hyper;
use hyper::server::Http;

extern crate hyper_fs;
use hyper_fs::{Config, StaticIndex};

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
    let config = Rc::new(Config::new().show_index(true).follow_links(true));    

    let mut server = Http::new()
        .bind(&addr, move || {
            Ok({
               StaticIndex::new(&index, config.clone())
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
