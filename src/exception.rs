use hyper::server::{Request, Response, Service};
use futures::future::{self, FutureResult};
use hyper::{Error, StatusCode};

use std::io::{self, ErrorKind as IoErrorKind};
use std::cell::Cell;
use std::fmt;
///1. HTTP Method is not `GET` or `HEAD`.
///
///2. `io::error`: not found, permission denied...
///
///3. `StaticFs`'s base url is not a prefix of `Request`'s path.
#[derive(Default)]
pub struct ExceptionHandler {
    error: Cell<Option<io::Error>>,
}
impl fmt::Debug for ExceptionHandler {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ExceptionHandler: {{error: Cell<Option<io::Error>>}}")
    }
}

/// Catch `io::Error` if it occurs
pub trait ExceptionCatcher {
    fn catch(&self, e: io::Error);
}

impl ExceptionCatcher for ExceptionHandler {
    fn catch(&self, e: io::Error) {
        self.error.set(Some(e))
    }
}

impl Service for ExceptionHandler {
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = FutureResult<Self::Response, Self::Error>;
    fn call(&self, _req: Self::Request) -> Self::Future {
        let error = self.error.replace(None);
        match error {
            Some(e) => match e.kind() {
                IoErrorKind::NotFound => future::ok(Response::new().with_status(StatusCode::NotFound)),
                IoErrorKind::PermissionDenied => future::ok(Response::new().with_status(StatusCode::Forbidden)),
                _ => future::err(Error::Io(e)),
            },
            None => future::ok(Response::new().with_status(StatusCode::MethodNotAllowed)),
        }
    }
}
