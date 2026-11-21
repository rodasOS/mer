use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::protocol::{IpcError, Request, Response, decode_response, encode_request};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Longer than the server's 2 s wait for a `Ctl` reply, so its "timed out" answer can arrive.
const READ_TIMEOUT: Duration = Duration::from_millis(3500);
const MAX_LINE: u64 = 1 << 20;

/// Blocking client for the running instance.
#[derive(Debug)]
pub struct Client;

impl Client {
    /// Sends one request and waits for its response.
    ///
    /// # Errors
    /// Fails when the socket cannot be reached or the exchange times out or is malformed.
    pub fn send(path: &Path, request: &Request) -> Result<Response, IpcError> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.write_all(&encode_request(request)?)?;
        stream.flush()?;
        let mut line = String::new();
        BufReader::new(stream.take(MAX_LINE)).read_line(&mut line)?;
        if line.is_empty() {
            return Err(IpcError::Protocol("connection closed without a response".to_owned()));
        }
        decode_response(&line)
    }

    /// True only when a `Ping` gets `Pong`.
    #[must_use]
    pub fn is_running(path: &Path) -> bool {
        matches!(Self::send(path, &Request::Ping), Ok(Response::Pong))
    }
}
