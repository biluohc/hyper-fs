use hyper::server::{Request, Response};
use hyper::{Error, StatusCode};

use super::{box_err, box_ok, FutureResponse};

use std::io::{self, ErrorKind as IoErrorKind};
use std::fmt;

/// `Exception`: As same as `Error`.
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
    fn call<E>(&self, e: E, req: Request) -> FutureResponse
    where
        E: Into<Exception>;
}

impl ExceptionHandlerService for ExceptionHandler {
    fn call<E>(&self, e: E, _req: Request) -> FutureResponse
    where
        E: Into<Exception>,
    {
        use Exception::*;
        match e.into() {
            Io(i) => match i.kind() {
                IoErrorKind::NotFound => box_ok(Response::new().with_status(StatusCode::NotFound)),
                IoErrorKind::PermissionDenied => box_ok(Response::new().with_status(StatusCode::Forbidden)),
                _ => box_err(Error::Io(i)),
            },
            Method => box_ok(Response::new().with_status(StatusCode::MethodNotAllowed)),
            Typo | Route => box_ok(Response::new().with_status(StatusCode::InternalServerError)),
        }
    }
}
