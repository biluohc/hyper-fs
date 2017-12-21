extern crate bytes;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate tokio_core;
extern crate url;
extern crate walkdir;

use futures::future::Future;
use hyper::{Error, Response};

/// `Box<Future<Item = Response, Error = Error>>`
pub type FutureObject = Box<Future<Item = Response, Error = Error>>;
// #[doc(hidden)]

pub(crate) mod exception;
pub(crate) mod static_file;
pub(crate) mod static_index;
pub(crate) mod static_fs;
pub(crate) mod config;

pub use exception::{Exception, ExceptionHandler, ExceptionHandlerService, ExceptionHandlerServiceAsync};
pub use static_index::StaticIndex;
pub use static_file::StaticFile;
pub use static_fs::StaticFs;
pub use config::Config;
