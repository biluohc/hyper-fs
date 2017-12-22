use hyper::server::{Request, Response};
use hyper::{Error, StatusCode};
use futures::future;

use super::FutureObject;

use std::io::{self, ErrorKind as IoErrorKind};
use std::fmt;

/// `Exception`: `Error` wrapped.
#[derive(Debug)]
pub enum Exception {
    ///`Io(io::error)`: not found, permission denied...
    Io(io::Error),
    /// HTTP Method is not `GET` or `HEAD`.
    Method,
    /// New a `StaticFile` by a index(Not is file) or New a `StaticIndex` by a file(Not is index).
    Typo,
    /// `StaticFs`'s base url is not a prefix of `Request`'s path.
    Route,
}

impl Exception {
    /// fast creat `Exception::Io(io::Error::from(IoErrorKind::NotFound))`
    pub fn not_found() -> Self {
        Exception::Io(io::Error::from(IoErrorKind::NotFound))
    }
}

impl Into<Exception> for io::Error {
    fn into(self) -> Exception {
        Exception::Io(self)
    }
}

/// Default `ExceptionHandler`
///
/// You can impl `ExceptionHandlerService` for your owner type, and use it by `with_handler`.
#[derive(Default, Debug, Clone)]
pub struct ExceptionHandler;

/// handle `Exception` and `return` `Response` if it occurs.
pub trait ExceptionHandlerService: fmt::Debug {
    fn call<E>(&self, e: E, req: Request) -> Result<Response, Error>
    where
        E: Into<Exception>;
}

impl ExceptionHandlerService for ExceptionHandler {
    fn call<E>(&self, e: E, _req: Request) -> Result<Response, Error>
    where
        E: Into<Exception>,
    {
        use Exception::*;
        match e.into() {
            Io(i) => match i.kind() {
                IoErrorKind::NotFound => Ok(Response::new().with_status(StatusCode::NotFound)),
                IoErrorKind::PermissionDenied => Ok(Response::new().with_status(StatusCode::Forbidden)),
                _ => Ok(Response::new().with_status(StatusCode::InternalServerError)),
            },
            Method => Ok(Response::new().with_status(StatusCode::MethodNotAllowed)),
            Typo | Route => Ok(Response::new().with_status(StatusCode::InternalServerError)),
        }
    }
}

/// Auto impl...
pub trait ExceptionHandlerServiceAsync: ExceptionHandlerService {
    fn call_async<E>(&self, e: E, req: Request) -> FutureObject
    where
        E: Into<Exception>;
}

impl<T> ExceptionHandlerServiceAsync for T
where
    T: ExceptionHandlerService,
{
    fn call_async<E>(&self, e: E, req: Request) -> FutureObject
    where
        E: Into<Exception>,
    {
        match self.call(e, req) {
            Ok(res) => Box::new(future::ok(res)),
            Err(e) => Box::new(future::err(e)),
        }
    }
}
