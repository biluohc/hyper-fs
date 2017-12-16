use futures::{future, Async, Future, Poll, Sink, Stream};
use futures::sync::mpsc::{self, Receiver, Sender};
use futures::future::FutureResult;
use hyper::{header, Chunk, Error, Method, StatusCode};
use hyper::server::{Request, Response, Service};

use super::{ExceptionCatcher, ExceptionHandler};
use super::Config;
use super::pool::{FsPool,FileRead};

use std::fs::{self, File};
use std::path::PathBuf;
use std::{mem, time};
use std::borrow::BorrowMut;
use std::sync::Arc;

/// Return a `Response` from a `File`
///
/// Todo: HTTP Bytes
pub struct StaticFile<EH = ExceptionHandler> {
    pool: Arc<FsPool>,
    file: PathBuf,
    handler: EH,
    config: Box<Config>,
}

impl<EH: ExceptionCatcher> StaticFile<EH> {
    pub fn with_handler<P: Into<PathBuf>>(pool: &Arc<FsPool>, file: P, handler: EH) -> Self {
        Self {
            pool: pool.clone(),
            file: file.into(),
            handler: handler,
            config: Box::new(Config::new()),
        }
    }
    pub fn set_config(&mut self, config: Config) {
        *self.config.borrow_mut() = config;
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
}
impl StaticFile<ExceptionHandler> {
    pub fn new<P: Into<PathBuf>>(pool: &Arc<FsPool>, file: P) -> Self {
        Self::with_handler(pool, file, ExceptionHandler::default())
    }
}
impl<EH: ExceptionCatcher> Service for StaticFile<EH>
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
                self.handler.catch(e);
                return self.handler.call(req);
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
            Err(e) => {
                self.handler.catch(e);
                return self.handler.call(req);
            }
        };
        let http_last_modified = header::HttpDate::from(last_modified);

        if let Some(&header::IfModifiedSince(ref value)) = req.headers().get() {
            if http_last_modified <= *value {
                return future::ok(Response::new().with_status(StatusCode::NotModified));
            }
        }

        // response Header
        let size = metadata.len();
        let delta_modified = last_modified
            .duration_since(time::UNIX_EPOCH)
            .expect("SystemTime::duration_since() failed");

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

        if self.config().cache_secs != 0 {
            res.headers_mut().set(header::CacheControl(vec![
                header::CacheDirective::Public,
                header::CacheDirective::MaxAge(self.config().cache_secs),
            ]));
        }

        // response body  stream
        match *req.method() {
            Method::Head => {}
            Method::Get => {
                let file = match File::open(&self.file) {
                    Ok(file) => file,
                    Err(e) => {
                        self.handler.catch(e);
                        return self.handler.call(req);
                    }
                };
                let (job, body) = FileRead::new(file);
                self.pool.push(job);
                res.set_body(body);
            }
            _ => unreachable!(),
        }
        future::ok(res)
    }
}
