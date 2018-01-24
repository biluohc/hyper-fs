use hyper::server::{Request, Response};
use hyper::{Error as HyperError, StatusCode};

use std::io::{self, ErrorKind as IoErrorKind};

/// `Error` wrapped.
#[derive(Debug)]
pub enum Error {
    ///`Io(io::error)`: not found, permission denied...
    Io(io::Error),
    /// HTTP Method is not `GET` or `HEAD`.
    Method,
    /// New a `StaticFile` by a index(Not is file) or New a `StaticIndex` by a file(Not is index).
    Typo,
    /// `StaticFs`'s base url is not a prefix of `Request`'s path.
    Route,
}

impl Error {
    /// fast creat `Error::Io(io::Error::from(IoErrorKind::NotFound))`
    pub fn not_found() -> Self {
        Error::Io(io::Error::from(IoErrorKind::NotFound))
    }
}

impl Into<Error> for io::Error {
    fn into(self) -> Error {
        Error::Io(self)
    }
}

/// Default Error Handler function
pub fn error_handler(err_req: (Error, Request)) -> Result<Response, HyperError> {
    use Error::*;
    let (err, _req) = err_req;
    Ok(Response::new().with_status(match err {
        Io(i) => match i.kind() {
            IoErrorKind::NotFound => StatusCode::NotFound,
            IoErrorKind::PermissionDenied => StatusCode::Forbidden,
            _ => StatusCode::InternalServerError,
        },
        Method => StatusCode::MethodNotAllowed,
        Typo | Route => StatusCode::InternalServerError,
    }))
}
