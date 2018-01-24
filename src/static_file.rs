use futures::{Async, Future, Poll, Sink, Stream};
use futures::sync::mpsc::{SendError, Sender};
use hyper::{header, Body, Chunk, Error as HyperError, Headers, Method, StatusCode};
use hyper::server::{Request, Response};

use bytes::{BufMut, BytesMut};
use futures_cpupool::{CpuFuture, CpuPool};
use tokio_core::reactor::Handle;

use super::{Config, Error, FutureObject};

use std::io::{self, Read, Seek, SeekFrom};
use std::fs::{self, File, Metadata};
use std::path::PathBuf;
use std::{mem, time};

/// Static File
pub struct StaticFile<C> {
    inner: Option<Inner<C>>,
    content: Option<CpuFuture<(Response, Option<SendAllCallBackBox>), (Error, Request)>>,
    handle: Handle,
}

trait SendAll {
    fn send_all(self, handle: &Handle);
}

trait SendAllCallBack {
    fn send_all_call_back(self: Box<Self>, handle: &Handle);
}
impl<T: SendAll> SendAllCallBack for T {
    fn send_all_call_back(self: Box<Self>, handle: &Handle) {
        (*self).send_all(handle)
    }
}
type SendAllCallBackBox = Box<SendAllCallBack + Send + 'static>;

type HeaderMaker = FnMut(&mut File, &Metadata, &PathBuf, &Request, &mut header::Headers) -> io::Result<()> + Send + 'static;

pub struct Inner<C> {
    pool: CpuPool,
    file: PathBuf,
    config: C,
    headers: Option<header::Headers>,
    header_maker: Option<Box<HeaderMaker>>,
}

impl<C> StaticFile<C>
where
    C: AsRef<Config> + Send + 'static,
{
    pub fn new<P: Into<PathBuf>>(handle: Handle, pool: CpuPool, file: P, config: C) -> Self {
        let inner = Inner {
            pool: pool,
            file: file.into(),
            config: config,
            headers: Some(header::Headers::new()),
            header_maker: None,
        };
        Self {
            inner: Some(inner),
            content: None,
            handle: handle,
        }
    }
    ///  You should seek to 0 if you modify the File(Read or seek), You could not write or append it.
    ///
    ///  The `FnMut` will being calling before 200 when Get method(If Range or 304 status, it will not be calling).
    ///
    ///  You could set `Content-Type`, `Charset`, etc ...
    ///
    //   Warning: do not modify `Content-Length`, 'ETag', etc(Already in `Headers`)
    pub fn headers_maker<M>(&mut self, maker: M)
    where
        M: FnMut(&mut File, &Metadata, &PathBuf, &Request, &mut header::Headers) -> io::Result<()> + Send + 'static,
    {
        self.inner.as_mut().unwrap().header_maker = Some(Box::new(maker))
    }
    /// You can set the init `Haeders`
    ///
    /// Warning: not being covered by inner code.
    pub fn headers_mut(&mut self) -> &mut Option<Headers> {
        &mut self.inner.as_mut().unwrap().headers
    }
    pub fn call(mut self, pool: &CpuPool, req: Request) -> FutureObject {
        let inner = mem::replace(&mut self.inner, None).expect("Call twice");
        self.content = Some(pool.spawn_fn(move || inner.call(req)));
        Box::new(self)
    }
}

impl<C> Future for StaticFile<C> {
    type Item = Response;
    type Error = (Error, Request);
    fn poll(&mut self) -> Poll<Response, Self::Error> {
        match self.content
            .as_mut()
            .expect("Poll a empty StaticIndex(NotInit/AlreadyConsume)")
            .poll()
        {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready((response, Some(t)))) => {
                t.send_all_call_back(&self.handle);
                Ok(Async::Ready(response))
            }
            Ok(Async::Ready((response, None))) => Ok(Async::Ready(response)),
            Err(e) => Err(e),
        }
    }
}

impl<C> Inner<C>
where
    C: AsRef<Config>,
{
    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }
    fn call(mut self, mut req: Request) -> Result<(Response, Option<SendAllCallBackBox>), (Error, Request)> {
        let mut headers = mem::replace(&mut self.headers, None).unwrap_or_else(header::Headers::new);
        headers.set(header::AcceptRanges(vec![header::RangeUnit::Bytes]));
        if *self.config().get_cache_secs() != 0 {
            headers.set(header::CacheControl(vec![
                header::CacheDirective::Public,
                header::CacheDirective::MaxAge(*self.config().get_cache_secs()),
            ]));
        }

        // method error
        match *req.method() {
            Method::Head | Method::Get => {}
            _ => return Err((Error::Method, req)),
        }
        // io error
        let metadata = match fs::metadata(&self.file) {
            Ok(metada) => {
                if !metada.is_file() {
                    return Err((Error::Typo, req));
                }
                metada
            }
            Err(e) => {
                return Err((e.into(), req));
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
            return Ok((
                Response::new()
                    .with_status(StatusCode::MovedPermanently)
                    .with_headers(headers),
                None,
            ));
        }
        // HTTP Last-Modified
        let last_modified = match metadata.modified() {
            Ok(time) => time,
            Err(e) => {
                return Err((e.into(), req));
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
                if !etags.is_empty() && *self.config.as_ref().get_cache_secs() > 0 && etag == etags[0] {
                    return Ok((
                        Response::new()
                            .with_headers(headers)
                            .with_status(StatusCode::NotModified),
                        None,
                    ));
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
                let mut file = match File::open(&self.file) {
                    Ok(file) => file,
                    Err(e) => {
                        return Err((e.into(), req));
                    }
                };
                if self.header_maker.is_some() {
                    let mut maker = mem::replace(&mut self.header_maker, None).unwrap();
                    if let Err(e) = maker(
                        &mut file,
                        &metadata,
                        &self.file,
                        &req,
                        &mut res.headers_mut(),
                    ) {
                        return Err((e.into(), req));
                    }
                    // have to reset seek if moved...
                }

                let (sender, body) = Body::pair();
                res.set_body(body);
                Ok((
                    res,
                    Some(Box::new(FileChunkStream::new(
                        &self.pool,
                        sender,
                        file,
                        *self.config().get_chunk_size(),
                    )) as SendAllCallBackBox),
                ))
            }
            Method::Head => Ok((res, None)),
            _ => unreachable!(),
        }
    }
}

impl<C> Inner<C>
where
    C: AsRef<Config>,
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
    ) -> Result<Result<(Response, Option<SendAllCallBackBox>), (Error, Request)>, (Request, header::Headers)> {
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
                    (true, false) => if *self.config.as_ref().get_cache_secs() > 0 {
                        Ok(Ok((
                            Response::new()
                                .with_headers(headers)
                                .with_status(StatusCode::NotModified),
                            None,
                        )))
                    } else {
                        Err((req, headers))
                    },
                    (false, _) => {
                        // 200
                        Err((req, headers))
                    }
                }
            }
            None => {
                if valid_ranges.len() != ranges.len() {
                    Ok(Ok((
                        Response::new()
                            .with_headers(headers)
                            .with_status(StatusCode::RangeNotSatisfiable),
                        None,
                    )))
                } else {
                    Ok(self.build_range_response(valid_ranges, metadata, req, headers))
                }
            }
        }
    }
    fn build_range_response(
        &self,
        valid_ranges: Vec<(u64, u64)>,
        metadata: &Metadata,
        req: Request,
        mut headers: header::Headers,
    ) -> Result<(Response, Option<SendAllCallBackBox>), (Error, Request)> {
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
                        return Err((e.into(), req));
                    }
                };
                let (sender, body) = Body::pair();
                res.set_body(body);
                Ok((
                    res,
                    Some(Box::new(FileRangeChunkStream::new(
                        &self.pool,
                        sender,
                        file,
                        valid_ranges,
                        *self.config().get_chunk_size(),
                    )) as SendAllCallBackBox),
                ))
            }
            Method::Head => Ok((res, None)),
            _ => unreachable!(),
        }
    }
}

impl SendAll for FileChunkStream {
    fn send_all(mut self, handle: &Handle) {
        let sender = mem::replace(&mut self.sender, None).unwrap();
        handle.spawn(sender.send_all(self).map(|_| ()).map_err(|_| ()));
    }
}

type OptionFileChunk = Option<(File, Chunk)>;
struct FileChunkStream {
    inner: CpuFuture<OptionFileChunk, HyperError>,
    pool: CpuPool,
    chunk_size: usize,
    sender: Option<Sender<Result<Chunk, HyperError>>>,
}
impl FileChunkStream {
    fn new(pool: &CpuPool, sender: Sender<Result<Chunk, HyperError>>, file: File, chunk_size: usize) -> Self {
        let chunk = pool.spawn_fn(move || read_a_chunk(file, chunk_size));
        FileChunkStream {
            inner: chunk,
            chunk_size: chunk_size,
            pool: pool.clone(),
            sender: Some(sender),
        }
    }
}
impl Stream for FileChunkStream {
    type Item = Result<Chunk, HyperError>;
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

fn read_a_chunk(mut file: File, chunk_size: usize) -> Result<OptionFileChunk, HyperError> {
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
        Err(e) => Err(HyperError::Io(e)),
    }
}

impl SendAll for FileRangeChunkStream {
    fn send_all(mut self, handle: &Handle) {
        let sender = mem::replace(&mut self.sender, None).unwrap();
        handle.spawn(sender.send_all(self).map(|_| ()).map_err(|_| ()));
    }
}

type OptionFileRangeChunk = Option<(File, Vec<(u64, u64)>, Chunk)>;

struct FileRangeChunkStream {
    inner: CpuFuture<OptionFileRangeChunk, HyperError>,
    pool: CpuPool,
    chunk_size: usize,
    sender: Option<Sender<Result<Chunk, HyperError>>>,
}

impl FileRangeChunkStream {
    fn new(pool: &CpuPool, sender: Sender<Result<Chunk, HyperError>>, file: File, ranges: Vec<(u64, u64)>, chunk_size: usize) -> Self {
        let chunk = pool.spawn_fn(move || read_a_range_chunk(file, ranges, chunk_size));
        FileRangeChunkStream {
            inner: chunk,
            chunk_size: chunk_size,
            pool: pool.clone(),
            sender: Some(sender),
        }
    }
}
impl Stream for FileRangeChunkStream {
    type Item = Result<Chunk, HyperError>;
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
fn read_a_range_chunk(mut file: File, mut ranges: Vec<(u64, u64)>, chunk_size: usize) -> Result<OptionFileRangeChunk, HyperError> {
    if ranges.is_empty() {
        return Ok(None);
    }
    let range_size = (ranges[0].1 - ranges[0].0 + 1) as usize;
    let cap = if range_size <= chunk_size {
        range_size
    } else {
        chunk_size
    };
    let mut buf = BytesMut::with_capacity(cap);
    let mut count = 0;

    while count < chunk_size {
        let start = ranges[0].0;
        let range_size = (ranges[0].1 - ranges[0].0 + 1) as usize;
        let reserve_size = if range_size <= chunk_size {
            ranges.remove(0);
            range_size
        } else {
            ranges[0].0 += chunk_size as u64;
            chunk_size
        };
        buf.reserve(reserve_size);
        match file.seek(SeekFrom::Start(start)) {
            Ok(s) => debug_assert_eq!(s, start),
            Err(e) => return Err(HyperError::Io(e)),
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
            Err(e) => return Err(HyperError::Io(e)),
        }
    }
    let chunk = Chunk::from(buf.freeze());
    Ok(Some((file, ranges, chunk)))
}
