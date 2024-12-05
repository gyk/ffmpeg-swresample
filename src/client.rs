use std::io::prelude::*;

use anyhow::Result;
use interprocess::local_socket::{prelude::*, GenericFilePath, GenericNamespaced, Stream};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];

    let name = if GenericNamespaced::is_supported() {
        "ffmpeg-swresample.socks".to_ns_name::<GenericNamespaced>()?
    } else {
        "/tmp/ffmpeg-swresample.socks".to_fs_name::<GenericFilePath>()?
    };

    let mut buffer = Vec::with_capacity(8192);

    let mut conn = Stream::connect(name)?;

    conn.write_all(path.as_bytes())?;
    conn.write_all(b"\n")?;

    conn.read_to_end(&mut buffer)?;

    print!("Server answered: {}", buffer.len());
    Ok(())
}
