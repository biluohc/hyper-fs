extern crate bytes;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
// #[macro_use]
extern crate log;
extern crate tokio_core;
extern crate url;
extern crate walkdir;

pub(crate) mod exception;
pub(crate) mod static_file;
pub(crate) mod static_index;
pub(crate) mod static_fs;

pub use exception::{ExceptionCatcher, ExceptionHandler};
pub use static_index::StaticIndex;
pub use static_file::StaticFile;
pub use static_fs::StaticFs;

///public config for file/index/fs
#[derive(Default, Debug, Clone)]
pub struct Config {
    follow_links: bool, // default: false
    show_index: bool,   // if false/true, use StaticIndexEmpty/StaticIndex, default is false.
    hide_entry: bool,   // if hide, hide .xxx in index(html), dafault is false.
    cache_secs: u32,    // 0
}
impl Config {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Config {
    pub fn follow_links(mut self, follow_links: bool) -> Self {
        self.follow_links = follow_links;
        self
    }
    pub fn show_index(mut self, show_index: bool) -> Self {
        self.show_index = show_index;
        self
    }
    pub fn hide_entry(mut self, hide_entry: bool) -> Self {
        self.hide_entry = hide_entry;
        self
    }
    pub fn cache_secs(mut self, cache_secs: u32) -> Self {
        self.cache_secs = cache_secs;
        self
    }
}
impl Config {
    pub fn get_follow_links(&self) -> bool {
        self.follow_links
    }
    pub fn get_show_index(&self) -> bool {
        self.show_index
    }
    pub fn get_hide_entry(&self) -> bool {
        self.hide_entry
    }
    pub fn get_cache_secs(&self) -> &u32 {
        &self.cache_secs
    }
}

impl Config {
    pub fn set_follow_links(&mut self, follow_links: bool) {
        self.follow_links = follow_links;
    }
    pub fn set_show_index(&mut self, show_index: bool) {
        self.show_index = show_index;
    }
    pub fn set_hide_entry(&mut self, hide_entry: bool) {
        self.hide_entry = hide_entry;
    }
    pub fn set_cache_secs(&mut self, cache_secs: u32) {
        self.cache_secs = cache_secs;
    }
}
