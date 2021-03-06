use hyper::header::{self, Headers};
use hyper::Request;
use mime_guess;

use std::fs::{File, Metadata};
use std::path::PathBuf;
use std::io;

/// use [`mime_guess`](https://github.com/abonander/mime_guess) to add `Content-Type` for file.
pub fn maker(_file: &mut File, _metadata: &Metadata, path: &PathBuf, _req: &Request, headers: &mut Headers) -> io::Result<()> {
    let mime = mime_guess::guess_mime_type(path);
    trace!("{:?}", mime);
    headers.set(header::ContentType(mime));
    Ok(())
}
