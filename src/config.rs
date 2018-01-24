///public config for file/index/fs
#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) follow_links: bool, // default: false
    pub(crate) show_index: bool,   // if false/true, use StaticIndexEmpty/StaticIndex, default is false.
    pub(crate) hide_entry: bool,   // if hide, hide .xxx in index(html), dafault is false.
    pub(crate) cache_secs: u32,    // 0
    // how many bytes read(poll) for every time(affect mem occupy and speed), default is 16384(16k, N times of Fsblock size(4k?8k))
    pub(crate) chunk_size: usize,
}

impl Config {
    pub fn new() -> Self {
        Self {
            follow_links: false,
            show_index: false,
            hide_entry: false,
            cache_secs: 0,
            chunk_size: 16_384,
        }
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
    pub fn chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
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
    pub fn get_chunk_size(&self) -> &usize {
        &self.chunk_size
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
    pub fn set_chunk_size(&mut self, chunk_size: usize) {
        self.chunk_size = chunk_size
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

// deref recursion limit reached
impl AsRef<Config> for Config {
    fn as_ref(&self) -> &Config {
        self
    }
}
