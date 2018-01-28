extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate mime;
extern crate mime_guess;
extern crate mxo_env_logger;
extern crate reqwest;
extern crate tokio_core;
extern crate url;
extern crate walkdir;

use mxo_env_logger::*;
use futures_cpupool::{Builder, CpuPool};
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::TcpListener;
use futures::{Future, Stream};
use hyper::server::{Http, Request, Response, Service};
use hyper::header::{self, Headers};
use hyper::Error;
use url::percent_encoding::{percent_decode, percent_encode_byte};
use reqwest::Client;
use walkdir::WalkDir;

extern crate hyper_fs;
use hyper_fs::{error_handler, Config, HyperFutureObject, StaticFs};

use std::path::PathBuf;
use std::time::Instant;
use std::sync::Arc;
use std::rc::Rc;
use std::sync::mpsc::*;
use std::net::SocketAddr;
use std::thread::Builder as ThreadBuilder;

#[test]
fn main() {
    let (mp, sc) = channel();
    ThreadBuilder::new().spawn(move || serve(&mp)).unwrap();
    let addr = sc.recv().unwrap();
    let host = format!("http://{}", addr);
    let client = Client::new();

    test_items(&host, &client);

    // block
    // sc.recv().ok();
}

fn test_items(host: &str, client: &Client) {
    for entry in WalkDir::new("./").follow_links(true) {
        let entry = match entry {
            Ok(o) => o,
            Err(_e) => continue,
        };
        let (is_dir, path) = (
            entry.path().is_dir(),
            entry.path().to_string_lossy().into_owned().to_owned(),
        );
        let mut path = (&path[2..]).to_string();
        if path.is_empty() {
            path = "./".to_string();
        }
        if path.starts_with(".git") || path.starts_with("target") {
            continue;
        }
        debug!("{}: {}", is_dir, path);

        let addr = path_to_addr(is_dir, &path, host);
        if is_dir {
            test_dir_(&path, &addr, client.get(&addr).send())
        } else {
            test_file(&path, &addr, client.get(&addr).send())
        }
    }
}

fn test_file(path: &str, addr: &str, http_res: reqwest::Result<reqwest::Response>) {
    use std::io::Read;
    use std::fs::File;
    let mut fc = vec![];
    let fc_res = File::open(path).and_then(|mut p| p.read_to_end(&mut fc));
    info!(
        "test_file({} -> {}) -> {}",
        path,
        addr,
        fc_res.is_ok() == http_res.is_ok()
    );
    let (_fc_res, mut http_res) = (fc_res.unwrap(), http_res.unwrap());

    // success
    assert!(http_res.status().is_success());

    // AcceptRanges
    {
        let range_unit = http_res
            .headers()
            .get::<reqwest::header::AcceptRanges>()
            .unwrap();
        assert_eq!(range_unit.0, vec![reqwest::header::RangeUnit::Bytes]);
    }
    // ContentLength
    {
        let content_length = http_res
            .headers()
            .get::<reqwest::header::ContentLength>()
            .unwrap();
        assert_eq!(fc.len() as u64, content_length.0);
    }

    // ContentType
    {
        let content_type = http_res
            .headers()
            .get::<reqwest::header::ContentType>()
            .unwrap();
        let mime = mime_guess::guess_mime_type(path);
        assert_eq!(mime, content_type.0);
    }
    // content
    let mut tmp: Vec<u8> = vec![];
    http_res.copy_to(&mut tmp).unwrap();
    assert_eq!(fc, tmp, "file's content != http's response");
}

fn test_dir_(path: &str, addr: &str, http_res: reqwest::Result<reqwest::Response>) {
    let dir_res = std::fs::read_dir(path);
    info!(
        "test_dir_({} -> {}) -> {}",
        path,
        addr,
        dir_res.is_ok() == http_res.is_ok()
    );
    let (_dir_res, mut http_res) = (dir_res.unwrap(), http_res.unwrap());

    // success
    assert!(http_res.status().is_success());

    // ContentType
    {
        let content_type = http_res
            .headers()
            .get::<reqwest::header::ContentType>()
            .unwrap();
        assert_eq!(mime::TEXT_HTML_UTF_8, content_type.0);
    }
    // ContentLength
    let content_length = {
        *http_res
            .headers()
            .get::<reqwest::header::ContentLength>()
            .unwrap()
    };

    // Content
    let content_text = http_res.text().unwrap();
    assert_eq!(content_text.len() as u64, content_length.0);
}

fn path_to_addr(is_dir: bool, path: &str, host: &str) -> String {
    let path_dec = path.split('/')
        .flat_map(|c| {
            "/".chars()
                .chain(c.bytes().flat_map(|cc| percent_encode_byte(cc).chars()))
        })
        .collect::<String>();
    if is_dir {
        format!("{}{}/", host, path_dec)
    } else {
        format!("{}{}", host, path_dec)
    }
}

fn serve(mp: &Sender<SocketAddr>) {
    init().expect("Init Log Failed");

    let pool = Builder::new().pool_size(3).name_prefix("hyper-fs").create();
    let config = Config::new()
        .cache_secs(60)
        .follow_links(true)
        .show_index(true); // .chunk_size(8196)

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let listener = TcpListener::bind(&"0.0.0.0:0".to_owned().parse().unwrap(), &handle).unwrap();
    let addr = listener.local_addr().unwrap();
    mp.send(addr).unwrap();

    let fs_server = Rc::new(FileServer::new(handle.clone(), pool, "./", config));

    let http = Http::new();
    let server = listener.incoming().for_each(|(socket, addr)| {
        http.bind_connection(&handle, socket, addr, fs_server.clone());
        Ok(())
    });
    info!("Listening on http://{} with 1 thread.", addr);
    core.run(server).unwrap();
}

struct FileServer {
    handle: Handle,
    path: PathBuf,
    pool: CpuPool,
    headers_index: Option<Headers>,
    config: Arc<Config>,
}
impl FileServer {
    fn new<P: Into<PathBuf>>(handle: Handle, pool: CpuPool, path: P, config: Config) -> Self {
        Self {
            handle: handle,
            path: path.into(),
            pool: pool,
            config: Arc::new(config),
            headers_index: Some({
                let mut tmp = Headers::new();
                tmp.set(header::ContentType(mime::TEXT_HTML_UTF_8));
                tmp
            }),
        }
    }
}

impl Service for FileServer {
    type Request = Request;
    type Response = Response;
    type Error = Error;
    type Future = HyperFutureObject;
    fn call(&self, req: Request) -> Self::Future {
        let timer = Instant::now();
        let mut fs = StaticFs::new(
            self.handle.clone(),
            self.pool.clone(),
            "/",
            &self.path,
            self.config.clone(),
        );
        // add `Content-Type` for index, `StaticFs` alrendy add `Content-Type` by `content_type_maker`(mime_guess crate) default.
        // use `static_fs` or `StaticFile` or `static_file` directly if need to use custom `Content-Type` for file.
        *fs.headers_index_mut() = self.headers_index.clone();

        let fs = fs.call(req);

        Box::new(fs.or_else(error_handler).map(move |res_req| {
            // cost time
            let time = timer.elapsed();
            // let ms = (time.as_secs()* 1000) as u32 + time.subsec_millis();
            let ms = (time.as_secs() * 1000) as u32 + time.subsec_nanos() / 1_000_000;

            // log
            info!(
                "{}ms {} {} {}",
                ms,
                res_req.0.status().as_u16(),
                res_req.1.method(),
                percent_decode(res_req.1.path().as_bytes())
                    .decode_utf8()
                    .unwrap()
                    .into_owned()
                    .to_owned()
            );
            // extract response
            res_req.0
        }))
    }
}
