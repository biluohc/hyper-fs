#![allow(unused_imports)]

#[macro_use]
extern crate log;
extern crate mxo_env_logger;
use mxo_env_logger::*;
extern crate futures;
extern crate hyper;
extern crate poolite;
extern crate tokio_core;
extern crate url;

use futures::future::Executor;
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::TcpListener;
use futures::{future, Async, Future, Poll, Sink, Stream};
use futures::sync::mpsc::{self, Receiver, Sender};
use futures::future::FutureResult;
use hyper::{header, Chunk, Error, Method, StatusCode};
use hyper::server::{Request, Response, Service};
use hyper::server::Http;
use poolite::{Builder, Pool};

extern crate hyper_fs;
extern crate num_cpus;
use hyper_fs::{StaticFile, ThreadPool};

use std::path::PathBuf;
use std::fs::{self, File};
use std::io::{BufReader, ErrorKind as IoErrorKind, Read};
use std::{mem, time};
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
    let file = env::args().nth(2).unwrap_or_else(|| "fn.jpg".to_owned());

    let pool = Pool2(Arc::new(
        Builder::new()
            .max(num_cpus::get() + 1)
            .name("hyper-fs")
            .build()
            .unwrap(),
    ));

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
struct Pool2(Arc<Pool>);
impl ThreadPool for Pool2 {
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
