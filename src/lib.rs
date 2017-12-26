/*!

*/
extern crate bytes;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
#[cfg(feature = "default")]
extern crate mime_guess;
extern crate tokio_core;
extern crate url;
extern crate walkdir;

use futures::future::Future;
use hyper::{Error, Response};

/// `Box<Future<Item = Response, Error = Error>>`
pub type FutureObject = Box<Future<Item = Response, Error = Error>>;
// #[doc(hidden)]

#[cfg(feature = "default")]
pub(crate) mod content_type;
pub(crate) mod exception;
#[doc(hidden)]
#[macro_use]
pub mod static_file;
#[doc(hidden)]
#[macro_use]
pub mod static_index;
#[doc(hidden)]
#[macro_use]
pub mod static_fs;
pub(crate) mod config;

pub use exception::{Exception, ExceptionHandler, ExceptionHandlerService, ExceptionHandlerServiceAsync};
pub use static_index::StaticIndex;
pub use static_file::StaticFile;
pub use static_fs::StaticFs;
pub use config::Config;
#[cfg(feature = "default")]
pub use content_type::maker as content_type_maker;
