use std::io::{self, stdin, BufRead};
use std::path::PathBuf;
use std::thread;

use anyhow::Result;
use crossbeam_channel::{bounded, select, Receiver};
use ipc_channel::ipc::{self, *};

use crate::messages::*;

pub fn run() -> Result<()> {
    let (handshake_server, handshake_server_name) = IpcOneShotServer::new().unwrap();
    println!("handshake_server_name: {}", handshake_server_name);
    let exe_path = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(exe_path)
        .arg("--thumbnailer")
        .arg("--handshake-id")
        .arg(handshake_server_name)
        .spawn()?;

    let (_, req_tx): (_, IpcSender<Request>) = handshake_server.accept().unwrap();

    let (resp_tx, resp_rx): (IpcSender<Response>, IpcReceiver<Response>) = ipc::channel().unwrap();
    req_tx.send(Request::Handshake(resp_tx)).unwrap();

    let (ctrlc_tx, ctrlc_rx) = bounded(0);
    ctrlc::set_handler(move || {
        eprintln!("Ctrl+C pressed!");
        ctrlc_tx
            .send(())
            .expect("Could not send signal on channel.")
    })
    .expect("Error setting Ctrl+C handler");

    let stdin_rx = spawn_stdin_chan();

    loop {
        let line = select! {
            recv(ctrlc_rx) -> _signal => {
                break;
            },
            recv(stdin_rx) -> line => match line {
                Ok(line) => line.ok(),
                Err(_) => break,
            }
        };

        let Some(line) = line else {
            break;
        };

        req_tx
            .send(Request::MakeAudioThumb {
                path: PathBuf::from(line),
            })
            .unwrap();

        match resp_rx.recv() {
            Ok(response) => {
                println!("{:?}", response);
            }
            Err(e) => {
                println!("Error: {}", e);
                break;
            }
        }
    }

    let _ = child.wait();

    Ok(())
}

// ===== User input & Signal handling ===== //

fn spawn_stdin_chan() -> Receiver<io::Result<String>> {
    let (tx, rx) = bounded(0);
    thread::spawn(move || {
        let stdin = stdin();
        for line in stdin.lock().lines() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}
