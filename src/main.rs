#[macro_use]
extern crate log;
extern crate mxo_env_logger;
use mxo_env_logger::*;
extern crate poolite;
use poolite::{Builder, Pool};

extern crate futures;
extern crate hyper;
extern crate tokio_core;
extern crate url;

use tokio_core::reactor::Handle;
use tokio_core::reactor::Core;
use tokio_core::net::TcpListener;
use futures::{future, Async, Future, Poll, Sink, Stream};
use futures::sync::mpsc::{self, Receiver, Sender};
use futures::future::FutureResult;
use hyper::{header, Body, Chunk, Error, Method, StatusCode};
use hyper::server::{Request, Response, Service};
use hyper::server::Http;

use std::path::PathBuf;
use std::fs::{self, File};
use std::io::{BufReader, ErrorKind as IoErrorKind, Read};
use std::{mem, time};
use std::sync::Arc;


fn main() {
    init().expect("Init Log Failed");
    let addr = format!(
        "0.0.0.0:{}",
        std::env::args().nth(1).unwrap_or("8080".to_owned())
    ).parse()
        .unwrap();
    let file = std::env::args().nth(2).unwrap_or("fn.jpg".to_owned());
    let pool = Arc::new(Builder::new().max(32).build().unwrap());

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let listener = TcpListener::bind(&addr, &handle).unwrap();

    let http = Http::new();
    let server = listener.incoming().for_each(|(socket, addr)| {
        let static_file_server = StaticFile::new(&handle, &pool, &file);
        http.bind_connection(&handle, socket, addr, static_file_server);
        Ok(())
    });

    println!("Listening on http://{} with 1 thread.", addr);
    core.run(server).unwrap();
}



#[derive(Debug, Default)]
pub struct DefaultExceptionHandler;

// io::error: not found, permission deny, is dir, is socket...
impl Service for DefaultExceptionHandler {
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureResult<Self::Response, Self::Error>;
    fn call(&self, req: Self::Request) -> Self::Future {
        future::ok(Response::new().with_status(match req.method() {
            &Method::Head | &Method::Get => StatusCode::NotFound,
            _ => StatusCode::BadRequest,
        }))
    }
}

pub struct StaticFile<EH = DefaultExceptionHandler> {
    handle: Handle,
    pool: Arc<Pool>,
    file: PathBuf,
    handler: EH,
    cache_secs: u32,
}

impl<EH> StaticFile<EH> {
    pub fn with_handler<P: Into<PathBuf>>(
        handle: &Handle,
        pool: &Arc<Pool>,
        file: P,
        handler: EH,
    ) -> Self {
        Self {
            handle: handle.clone(),
            pool: pool.clone(),
            file: file.into(),
            handler: handler,
            cache_secs: 86400, // 1 day
        }
    }
}
impl StaticFile<DefaultExceptionHandler> {
    pub fn new<P: Into<PathBuf>>(handle: &Handle, pool: &Arc<Pool>, file: P) -> Self {
        Self::with_handler(handle, pool, file, DefaultExceptionHandler::default())
    }
}
impl<EH> Service for StaticFile<EH>
where
    EH: Service<
        Request = Request,
        Response = Response,
        Error = Error,
        Future = FutureResult<Response, Error>,
    >,
{
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureResult<Response, Error>;

    fn call(&self, req: Request) -> Self::Future {
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return self.handler.call(req),
        }

        // io error
        let metadata = match fs::metadata(&self.file) {
            Ok(metada) => {
                if !metada.is_file() {
                    return self.handler.call(req);
                }
                metada
            }
            Err(e) => {
                return match e.kind() {
                    IoErrorKind::NotFound => self.handler.call(req),
                    IoErrorKind::PermissionDenied => {
                        future::ok(Response::new().with_status(StatusCode::Forbidden))
                    }
                    _ => future::err(Error::Io(e)),
                };
            }
        };

        //301, redirect
        // https://rust-lang.org/logo.ico///?labels=E-easy&state=open
        // http://0.0.0.0:8000///
        if req.path().len() != 1 && req.path().ends_with('/') {
            let mut new_path = req.path().to_owned();
            while new_path.ends_with('/') {
                new_path.pop();
            }
            if new_path.is_empty() {
                new_path.push('/');
            }
            if let Some(query) = req.query() {
                new_path.push('?');
                new_path.push_str(query);
            }
            return future::ok(
                Response::new()
                    .with_status(StatusCode::MovedPermanently)
                    .with_header(header::Location::new(new_path)),
            );
        }

        // HTTP Last-Modified
        let last_modified = match metadata.modified() {
            Ok(time) => time,
            Err(err) => return future::err(Error::Io(err)),
        };
        let http_last_modified = header::HttpDate::from(last_modified);

        if let Some(&header::IfModifiedSince(ref value)) = req.headers().get() {
            println!("m<=b : {:?}", http_last_modified <= *value);
            if http_last_modified <= *value {
                return future::ok(Response::new().with_status(StatusCode::NotModified));
            }
        }

        // response Header
        let size = metadata.len();
        let delta_modified = last_modified.duration_since(time::UNIX_EPOCH).expect(
            "SystemTime::duration_since() failed",
        );

        let etag = format!(
            "{:x}-{:x}.{:x}",
            size,
            delta_modified.as_secs(),
            delta_modified.subsec_nanos()
        );
        let mut res = Response::new()
            .with_header(header::ContentLength(size))
            .with_header(header::LastModified(http_last_modified))
            .with_header(header::ETag(header::EntityTag::weak(etag)));

        if self.cache_secs != 0 {
            res.headers_mut().set(header::CacheControl(vec![
                header::CacheDirective::Public,
                header::CacheDirective::MaxAge(self.cache_secs),
            ]));
        }

        // response body  stream
        match req.method() {
            &Method::Head => {}
            &Method::Get => {
                let file = match File::open(&self.file) {
                    Ok(file) => file,
                    Err(err) => return future::err(Error::Io(err)),
                };
                res.set_body(body_file_init(file, self.pool.clone()));
            }
            _ => unreachable!(),
        }
        future::ok(res)
    }
}

fn body_file_init(file: File, pool: Arc<Pool>) -> Receiver<Result<Chunk, Error>> {
    let file = BufReader::new(file);
    let buf: [u8; 8192] = unsafe { mem::uninitialized() };
    let full = false;
    let len = 0;
    let (sender, body) = mpsc::channel::<Result<Chunk, Error>>(128);
    let pool_new = pool.clone();
    pool.push(move || file_nb_read(file, sender, pool_new, full, len, buf));
    body
}

fn file_nb_read(
    mut file: BufReader<File>,
    mut sender: Sender<Result<Chunk, Error>>,
    pool: Arc<Pool>,
    mut full: bool,
    mut len: usize,
    mut buf: [u8; 8192],
) {
    loop {
        if full {
            let vec = (&buf[0..len]).to_vec();
            if let Err(e) = sender.try_send(Ok(Chunk::from(vec))) {
                if e.is_full() {
                    full = true;
                    let pool_new = pool.clone();
                    pool.push(move || file_nb_read(file, sender, pool_new, full, len, buf));
                }
                // receiver break connection
                trace!("try_send body's chunks failed: {:?}", e);
                return;
            }
            len = 0;
            full = false;
        }
        match file.read(&mut buf) {
            Ok(len_) => {
                if len_ == 0 {
                    return;
                }
                let vec = (&buf[0..len_]).to_vec();
                if let Err(e) = sender.try_send(Ok(Chunk::from(vec))) {
                    if e.is_full() {
                        full = true;
                        len = len_;
                        let pool_new = pool.clone();
                        pool.push(move || file_nb_read(file, sender, pool_new, full, len, buf));
                    }
                    // receiver break connection
                    trace!("try_send body's chunks failed: {:?}", e);
                    return;
                }
            }
            Err(e) => {
                    // todo, resend...
                if let Err(e) = sender.try_send(Err(Error::Io(e))) {
                    // receiver break connection
                    trace!("try_send body's chunks failed: {:?}", e);
                    return;
                }
                return;
            }
        }
    }
}
