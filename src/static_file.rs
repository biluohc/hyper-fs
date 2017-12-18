use futures::{Async, Future, Poll, Sink, Stream};
use futures::sync::mpsc::SendError;
use hyper::{header, Body, Chunk, Error, Headers, Method, StatusCode};
use hyper::server::{Request, Response, Service};

use bytes::{BufMut, BytesMut};
use futures_cpupool::{CpuFuture, CpuPool};
use tokio_core::reactor::Handle;

use super::{Exception, ExceptionHandler, ExceptionHandlerService};
use super::{box_ok, FutureResponse};
use super::Config;

use std::io::{Read, Seek, SeekFrom};
use std::fs::{self, File, Metadata};
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

impl<C, EH> StaticFile<C, EH>
where
    C: AsRef<Config>,
    EH: ExceptionHandlerService,
{
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
impl<C> StaticFile<C, ExceptionHandler>
where
    C: AsRef<Config>,
{
    pub fn new<P: Into<PathBuf>>(handle: Handle, pool: CpuPool, file: P, config: C) -> Self {
        Self::with_handler(handle, pool, file, config, ExceptionHandler::default())
    }
}
impl<C, EH> Service for StaticFile<C, EH>
where
    C: AsRef<Config>,
    EH: ExceptionHandlerService,
{
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureResponse;

    fn call(&self, mut req: Request) -> Self::Future {
        let mut headers = Headers::new();
        headers.set(header::AcceptRanges(vec![header::RangeUnit::Bytes]));
        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return self.handler.call(Exception::Method, req),
        }

        if self.config().cache_secs != 0 {
            headers.set(header::CacheControl(vec![
                header::CacheDirective::Public,
                header::CacheDirective::MaxAge(self.config().cache_secs),
            ]));
        }

        // io error
        let metadata = match fs::metadata(&self.file) {
            Ok(metada) => {
                if !metada.is_file() {
                    return self.handler.call(Exception::Typo, req);
                }
                metada
            }
            Err(e) => {
                return self.handler.call(e, req);
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
            headers.set(header::Location::new(new_path));
            return box_ok(
                Response::new()
                    .with_status(StatusCode::MovedPermanently)
                    .with_headers(headers),
            );
        }

        // HTTP Last-Modified
        let last_modified = match metadata.modified() {
            Ok(time) => time,
            Err(e) => {
                return self.handler.call(e, req);
            }
        };
        let delta_modified = last_modified
            .duration_since(time::UNIX_EPOCH)
            .expect("SystemTime::duration_since(UNIX_EPOCH) failed");
        let http_last_modified = header::HttpDate::from(last_modified);

        let size = metadata.len();
        let etag = header::EntityTag::weak(format!(
            "{:x}-{:x}.{:x}",
            size,
            delta_modified.as_secs(),
            delta_modified.subsec_nanos()
        ));
        headers.set(header::LastModified(http_last_modified));
        headers.set(header::ETag(etag.clone()));

        // Range
        let range: Option<header::Range> = req.headers_mut().remove();
        let (req, mut headers) = if let Some(header::Range::Bytes(ranges)) = range {
            match self.range(
                &ranges,
                req,
                headers,
                &metadata,
                &last_modified,
                &delta_modified,
                &etag,
            ) {
                Ok(o) => return o,
                Err(rh) => rh,
            }
        } else {
            // 304
            if let Some(&header::IfNoneMatch::Items(ref etags)) = req.headers().get() {
                if !etags.is_empty() && etag == etags[0] {
                    return box_ok(
                        Response::new()
                            .with_headers(headers)
                            .with_status(StatusCode::NotModified),
                    );
                }
            }
            (req, headers)
        };

        // 200
        // response Header
        headers.set(header::ContentLength(size));
        let mut res = Response::new().with_headers(headers);
        // response body  stream
        match *req.method() {
            Method::Get => {
                let file = match File::open(&self.file) {
                    Ok(file) => file,
                    Err(e) => {
                        return self.handler.call(e, req);
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
            Method::Head => {}
            _ => unreachable!(),
        }
        box_ok(res)
    }
}

impl<C, EH> StaticFile<C, EH>
where
    C: AsRef<Config>,
    EH: ExceptionHandlerService,
{
    fn range(
        &self,
        ranges: &Vec<header::ByteRangeSpec>,
        req: Request,
        headers: header::Headers,
        metadata: &Metadata,
        last_modified: &time::SystemTime,
        delta_modified: &time::Duration,
        etag: &header::EntityTag,
    ) -> Result<FutureResponse, (Request, header::Headers)> {
        let valid_ranges: Vec<_> = ranges
            .as_slice()
            .iter()
            .filter_map(|r| r.to_satisfiable_range(metadata.len()))
            .collect();

        let not_modified = if let Some(&header::IfRange::EntityTag(ref e)) = req.headers().get() {
            Some(e == etag)
        } else if let Some(&header::IfRange::Date(ref d)) = req.headers().get() {
            let http_last_modified_sub_nsecs = header::HttpDate::from(*last_modified - time::Duration::new(0, delta_modified.subsec_nanos()));
            Some(http_last_modified_sub_nsecs <= *d)
        } else {
            None
        };
        match not_modified {
            Some(not_modified) => {
                match (not_modified, valid_ranges.len() == ranges.len()) {
                    (true, true) => Ok(self.build_range_response(valid_ranges, metadata, req, headers)),
                    (true, false) => Ok(box_ok(
                        Response::new()
                            .with_headers(headers)
                            .with_status(StatusCode::NotModified),
                    )),
                    (false, _) => {
                        // 200
                        Err((req, headers))
                    }
                }
            }
            None => {
                if valid_ranges.len() != ranges.len() {
                    Ok(box_ok(
                        Response::new()
                            .with_headers(headers)
                            .with_status(StatusCode::RangeNotSatisfiable),
                    ))
                } else {
                    Ok(self.build_range_response(valid_ranges, metadata, req, headers))
                }
            }
        }
    }
    fn build_range_response(&self, valid_ranges: Vec<(u64, u64)>, metadata: &Metadata, req: Request, mut headers: header::Headers) -> FutureResponse {
        let content_length = valid_ranges
            .as_slice()
            .iter()
            .fold(0u64, |len, &(a, b)| len + b - a + 1);
        // accept-ranges: bytes
        // content-range: bytes 2001-4285/4286
        let content_ranges = {
            let mut s = "bytes ".to_owned();
            for (idx, &(a, b)) in valid_ranges.as_slice().iter().enumerate() {
                if idx + 1 == valid_ranges.len() {
                    s.push_str(&format!("{}-{}", a, b));
                } else {
                    s.push_str(&format!("{}-{},", a, b));
                }
            }
            s.push_str(&format!("/{}", metadata.len()));
            s
        };
        headers.set(header::ContentLength(content_length));
        headers.set_raw("content-ranges", content_ranges);
        let mut res = Response::new()
            .with_status(StatusCode::PartialContent)
            .with_headers(headers);
        match *req.method() {
            Method::Get => {
                let file = match File::open(&self.file) {
                    Ok(file) => file,
                    Err(e) => {
                        return self.handler.call(e, req);
                    }
                };
                let (sender, body) = Body::pair();
                self.handle.spawn(
                    sender
                        .send_all(FileRangeChunkStream::new(
                            &self.pool,
                            file,
                            valid_ranges,
                            *self.config().get_chunk_size(),
                        ))
                        .map(|_| ())
                        .map_err(|_| ()),
                );
                res.set_body(body);
            }
            Method::Head => {}
            _ => unreachable!(),
        }
        box_ok(res)
    }
}

type OptionFileChunk = Option<(File, Chunk)>;
struct FileChunkStream {
    inner: CpuFuture<OptionFileChunk, Error>,
    pool: CpuPool,
    chunk_size: usize,
}
impl FileChunkStream {
    fn new(pool: &CpuPool, file: File, chunk_size: usize) -> Self {
        let chunk = pool.spawn_fn(move || read_a_chunk(file, chunk_size));
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

fn read_a_chunk(mut file: File, chunk_size: usize) -> Result<OptionFileChunk, Error> {
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

type OptionFileRangeChunk = Option<(File, Vec<(u64, u64)>, Chunk)>;

struct FileRangeChunkStream {
    inner: CpuFuture<OptionFileRangeChunk, Error>,
    pool: CpuPool,
    chunk_size: usize,
}

impl FileRangeChunkStream {
    fn new(pool: &CpuPool, file: File, ranges: Vec<(u64, u64)>, chunk_size: usize) -> Self {
        let chunk = pool.spawn_fn(move || read_a_range_chunk(file, ranges, chunk_size));
        FileRangeChunkStream {
            inner: chunk,
            chunk_size: chunk_size,
            pool: pool.clone(),
        }
    }
}
impl Stream for FileRangeChunkStream {
    type Item = Result<Chunk, Error>;
    type Error = SendError<Self::Item>;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(Some((file, ranges, chunk)))) => {
                let chunk_size = self.chunk_size;
                let new_chunk = self.pool
                    .spawn_fn(move || read_a_range_chunk(file, ranges, chunk_size));
                self.inner = new_chunk;
                Ok(Async::Ready(Some(Ok(chunk))))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Err(e) => Ok(Async::Ready(Some(Err(e)))),
        }
    }
}
fn read_a_range_chunk(mut file: File, mut ranges: Vec<(u64, u64)>, chunk_size: usize) -> Result<OptionFileRangeChunk, Error> {
    if ranges.is_empty() {
        return Ok(None);
    }
    let mut buf = BytesMut::new();
    let mut count = 0;

    while count < chunk_size {
        let start = ranges[0].0;
        let _range_size = (ranges[0].1 - ranges[0].0 + 1) as usize;
        let reserve_size = if _range_size <= chunk_size {
            ranges.remove(0);
            _range_size
        } else {
            ranges[0].0 += chunk_size as u64;
            chunk_size
        };
        buf.reserve(reserve_size);
        match file.seek(SeekFrom::Start(start)) {
            Ok(s) => debug_assert_eq!(s, start),
            Err(e) => return Err(Error::Io(e)),
        }
        match file.read(unsafe { buf.bytes_mut() }) {
            Ok(len) => {
                // ?
                if len < reserve_size {
                    unreachable!()
                }
                unsafe {
                    buf.advance_mut(reserve_size);
                }
                if ranges.is_empty() {
                    break;
                }
                count += reserve_size;
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
    let chunk = Chunk::from(buf.freeze());
    Ok(Some((file, ranges, chunk)))
}
