use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::client::Client;
use crate::protocol::{IpcError, Request, Response, decode_request, encode_response};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_LINE: u64 = 1 << 20;

/// A request delivered to the owner of the [`Server`].
#[derive(Debug)]
pub struct Incoming {
    pub request: Request,
    reply: Option<mpsc::Sender<Response>>,
}

impl Incoming {
    /// Answers the request. A no-op for requests that were already answered (`Ping`, `Hook`).
    pub fn reply(self, response: Response) {
        if let Some(tx) = self.reply {
            let _ = tx.send(response);
        }
    }
}

/// The listening socket. Removes the socket file when dropped.
#[derive(Debug)]
pub struct Server {
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Binds the socket and starts serving; requests arrive on the returned receiver.
    ///
    /// # Errors
    /// [`IpcError::AlreadyRunning`] when a live instance owns the socket, or an i/o error.
    pub fn bind(path: &Path) -> Result<(Self, UnboundedReceiver<Incoming>), IpcError> {
        if let Some(dir) = path.parent() {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
            // A `mer` directory made by an older version, or by hand, may be looser than 0700.
            // Only our own directory is touched: `MER_SOCKET` may point anywhere.
            if dir.file_name().is_some_and(|name| name == "mer") {
                let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
            }
        }
        if path.exists() {
            if Client::is_running(path) {
                return Err(IpcError::AlreadyRunning);
            }
            fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        let (tx, rx) = unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let accept_stop = Arc::clone(&stop);
        thread::Builder::new().name("mer-ipc-accept".to_owned()).spawn(move || {
            for stream in listener.incoming() {
                if accept_stop.load(Ordering::Acquire) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let tx = tx.clone();
                let _ = thread::Builder::new()
                    .name("mer-ipc-conn".to_owned())
                    .spawn(move || handle(stream, &tx));
            }
        })?;
        Ok((Self { path: path.to_owned(), stop }, rx))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the blocked accept loop so its thread exits.
        let _ = UnixStream::connect(&self.path);
        let _ = fs::remove_file(&self.path);
    }
}

fn handle(stream: UnixStream, tx: &UnboundedSender<Incoming>) {
    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return;
    }
    let mut line = String::new();
    let read = match stream.try_clone() {
        Ok(reader) => BufReader::new(reader.take(MAX_LINE)).read_line(&mut line),
        Err(_) => return,
    };
    let response = if read.is_err() || line.is_empty() {
        Response::Error("no request received".to_owned())
    } else {
        match decode_request(&line) {
            Err(response) => response,
            Ok(Request::Ping) => Response::Pong,
            Ok(request @ Request::Hook { .. }) => {
                let _ = tx.unbounded_send(Incoming { request, reply: None });
                Response::Ok
            }
            Ok(request @ Request::Ctl(_)) => {
                let (reply_tx, reply_rx) = mpsc::channel();
                if tx.unbounded_send(Incoming { request, reply: Some(reply_tx) }).is_err() {
                    Response::Error("server is shutting down".to_owned())
                } else {
                    reply_rx
                        .recv_timeout(REPLY_TIMEOUT)
                        .unwrap_or_else(|_| Response::Error("timed out".to_owned()))
                }
            }
        }
    };
    let mut stream = stream;
    let _ = stream.write_all(&encode_response(&response));
    let _ = stream.flush();
}
