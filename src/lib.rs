extern crate bytes;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate tokio_core;
extern crate url;
extern crate walkdir;

use futures::future::{self, FutureResult};
use hyper::{Error, Response};

pub type FutureResponse = Box<FutureResult<Response, Error>>;

#[doc(hidden)]
pub fn box_ok(r: Response) -> FutureResponse {
    Box::new(future::ok(r))
}
#[doc(hidden)]
pub fn box_err(e: Error) -> FutureResponse {
    Box::new(future::err(e))
}

pub(crate) mod exception;
pub(crate) mod static_file;
pub(crate) mod static_index;
pub(crate) mod static_fs;
pub(crate) mod config;

pub use exception::{Exception, ExceptionHandler, ExceptionHandlerService};
pub use static_index::StaticIndex;
pub use static_file::StaticFile;
pub use static_fs::StaticFs;
pub use config::Config;
