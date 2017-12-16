use futures::{future, Async, Future, Poll, Sink, Stream};
use futures::sync::mpsc::SendError;
use futures::future::FutureResult;
use hyper::{header, Body, Chunk, Error, Method, StatusCode};
use hyper::server::{Request, Response, Service};

use bytes::{BufMut, BytesMut};
use futures_cpupool::{CpuFuture, CpuPool};
use tokio_core::reactor::Handle;

use super::{ExceptionCatcher, ExceptionHandler};
use super::Config;

use std::io::{BufReader, Read};
use std::fs::{self, File};
use std::path::PathBuf;
use std::time;

/// Return a `Response` from a `File`
///
/// Todo: HTTP Bytes
pub struct StaticFile<C, EH = ExceptionHandler> {
    handle: Handle,
    pool: CpuPool,
    file: PathBuf,
    config: C,
    handler: EH,
}

impl<C: AsRef<Config>, EH: ExceptionCatcher> StaticFile<C, EH> {
    pub fn with_handler<P: Into<PathBuf>>(handle: Handle, pool: CpuPool, file: P, config: C, handler: EH) -> Self {
        Self {
            handle: handle,
            pool: pool,
            file: file.into(),
            handler: handler,
            config: config,
        }
    }
    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }
}
impl<C: AsRef<Config>> StaticFile<C, ExceptionHandler> {
    pub fn new<P: Into<PathBuf>>(handle: Handle, pool: CpuPool, file: P, config: C) -> Self {
        Self::with_handler(handle, pool, file, config, ExceptionHandler::default())
    }
}
impl<C: AsRef<Config>, EH: ExceptionCatcher> Service for StaticFile<C, EH>
where
    EH: Service<Request = Request, Response = Response, Error = Error, Future = FutureResult<Response, Error>>,
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
                let (sender, body) = Body::pair();
                self.handle.spawn(
                    sender
                        .send_all(FileChunkStream::new(
                            &self.pool,
                            file,
                            *self.config().get_chunk_size(),
                        ))
                        .map(|_| ())
                        .map_err(|_| ()),
                );
                res.set_body(body);
            }
            _ => unreachable!(),
        }
        future::ok(res)
    }
}

struct FileChunkStream {
    inner: CpuFuture<Option<(BufReader<File>, Chunk)>, Error>,
    pool: CpuPool,
    chunk_size: usize,
}
impl FileChunkStream {
    fn new(pool: &CpuPool, file: File, chunk_size: usize) -> Self {
        let chunk = pool.spawn_fn(move || read_a_chunk(BufReader::new(file), chunk_size));
        FileChunkStream {
            inner: chunk,
            chunk_size: chunk_size,
            pool: pool.clone(),
        }
    }
}
impl Stream for FileChunkStream {
    type Item = Result<Chunk, Error>;
    type Error = SendError<Self::Item>;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(Some((file, chunk)))) => {
                let chunk_size = self.chunk_size;
                let new_chunk = self.pool.spawn_fn(move || read_a_chunk(file, chunk_size));
                self.inner = new_chunk;
                Ok(Async::Ready(Some(Ok(chunk))))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Err(e) => Ok(Async::Ready(Some(Err(e)))),
        }
    }
}

fn read_a_chunk(mut file: BufReader<File>, chunk_size: usize) -> Result<Option<(BufReader<File>, Chunk)>, Error> {
    let mut buf = BytesMut::with_capacity(chunk_size);
    match file.read(unsafe { buf.bytes_mut() }) {
        Ok(0) => Ok(None),
        Ok(len) => {
            unsafe {
                buf.advance_mut(len);
            }
            let chunk = Chunk::from(buf.freeze());
            Ok(Some((file, chunk)))
        }
        Err(e) => Err(Error::Io(e)),
    }
}
