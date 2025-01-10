mod master;
mod thumbnailer;

mod messages;

use clap::Parser;

#[derive(Parser)]
#[command(about)]
struct Args {
    /// Run as server
    #[arg(long)]
    thumbnailer: bool,
    #[arg(long)]
    handshake_id: Option<String>,
}

fn main() {
    let args = Args::parse();
    if args.thumbnailer {
        thumbnailer::run(args.handshake_id.unwrap()).unwrap();
    } else {
        master::run().unwrap();
    }
}
