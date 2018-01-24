/*!
[![Build status](https://travis-ci.org/biluohc/hyper-fs.svg?branch=master)](https://github.com/biluohc/hyper-fs)
[![Latest version](https://img.shields.io/crates/v/hyper-fs.svg)](https://crates.io/crates/hyper-fs)
[![All downloads](https://img.shields.io/crates/d/hyper-fs.svg)](https://crates.io/crates/hyper-fs)
[![Downloads of latest version](https://img.shields.io/crates/dv/hyper-fs.svg)](https://crates.io/crates/hyper-fs)
[![Documentation](https://docs.rs/hyper-fs/badge.svg)](https://docs.rs/hyper-fs)

## [hyper-fs](https://github.com/biluohc/hyper-fs)

Static File Service for hyper 0.11+.

### Usage

On Cargo.toml:

```toml
 [dependencies]
 hyper-fs = "0.2.0"
```

### Documentation
* Visit [Docs.rs](https://docs.rs/hyper-fs/)

or

* Run `cargo doc --open` after modified the toml file.

### Examples

* [examples/](https://github.com/biluohc/hyper-fs/tree/master/examples)

* [fht2p](https://github.com/biluohc/fht2p): the library write for it.

## To Do

| name | status |
| ------ | --- |
|Get/Head                  | yes |
|Not Modified(304)         | yes |
|File Range(bytes)         | yes |
|Upload                    | no  |
*/
extern crate bytes;
#[macro_use]
extern crate cfg_if;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate tokio_core;
extern crate url;
extern crate walkdir;

use futures::future::Future;
use hyper::{Error as HyperError, Request, Response};

/// `Box<Future<Item = Response, Error = (Error, Request)>>`
pub type FutureObject = Box<Future<Item = Response, Error = (Error, Request)>>;
/// `Box<Future<Item = Response, Error = HyperError>>`
pub type HyperFutureObject = Box<Future<Item = Response, Error = HyperError>>;
// #[doc(hidden)]

pub(crate) mod config;
pub(crate) mod error;
pub(crate) mod static_file;
pub(crate) mod static_index;

pub use config::Config;
pub use error::{error_handler, Error};
pub use static_index::StaticIndex;
pub use static_file::StaticFile;

cfg_if! {
    if #[cfg(feature = "default")] {
    extern crate mime_guess;
    pub(crate) mod content_type;
    pub(crate) mod static_fs;
    pub use static_fs::StaticFs;
    pub use content_type::maker as content_type_maker;
  }
}
