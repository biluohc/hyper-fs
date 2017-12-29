# hyper-fs

[![Build status](https://travis-ci.org/biluohc/hyper-fs.svg?branch=master)](https://github.com/biluohc/hyper-fs)
[![Latest version](https://img.shields.io/crates/v/hyper-fs.svg)](https://crates.io/crates/hyper-fs)
[![All downloads](https://img.shields.io/crates/d/hyper-fs.svg)](https://crates.io/crates/hyper-fs)
[![Downloads of latest version](https://img.shields.io/crates/dv/hyper-fs.svg)](https://crates.io/crates/hyper-fs)
[![Documentation](https://docs.rs/hyper-fs/badge.svg)](https://docs.rs/hyper-fs)

### [hyper-fs](https://github.com/biluohc/hyper-fs)

Static File Service for hyper 0.11+.

#### Usage

On Cargo.toml:

```toml
 [dependencies]
 hyper-fs = "0.1.1"
```

#### Documentation
* Visit [Docs.rs](https://docs.rs/hyper-fs/)

or

* Run `cargo doc --open` after modified the toml file.

#### Examples

* [examples/](https://github.com/biluohc/hyper-fs/tree/master/examples)

* [fht2p](https://github.com/biluohc/fht2p): the library write for it.

### To Do

| name | status |
| ------ | ---:|
|Get/Head                  | yes|
|Not Modified(304)         | yes|
|File Range(bytes)         | yes|
|Upload                    | no |

License: BSD-3-Clause
