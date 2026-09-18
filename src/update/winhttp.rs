//! The one real [`Feed`]: WinHTTP, the system's HTTP client for programs
//! with no user in front of them.
//!
//! Microsoft's library, the system certificate store, the system proxy
//! (`WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY`), no certificate check relaxed.
//! It follows `https` to `https` redirects on its own, which the asset chain
//! needs -- `github.com` to `release-assets.githubusercontent.com` -- and is
//! told not to for the one request whose redirect *is* the answer.
//!
//! Synchronous, on purpose: every call here runs on the updater's worker
//! thread, never on the thread that owns the window.

use std::ffi::c_void;
use std::io::Write;
use std::path::Path;

use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTP_PORT, INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
    WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
    WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR};

use super::Fault;
use super::feed::Feed;

/// Resolve, connect, send, receive: generous for a person waiting on a
/// click, short enough that "no connection" is said within the minute.
const TIMEOUTS_MS: (i32, i32, i32, i32) = (10_000, 10_000, 15_000, 30_000);

pub struct WinHttp {
    agent: String,
    /// `https` only, which is the shipped program. The tests build one that
    /// also speaks plain `http` to a listener of their own on `127.0.0.1`:
    /// a TLS server without a crate would be SChannel by hand, and TLS is
    /// WinHTTP's to get right -- a bad certificate was measured refused
    /// with 12175 on 2026-09-18.
    secure_only: bool,
    proxy: WINHTTP_ACCESS_TYPE,
    timeouts: (i32, i32, i32, i32),
}

impl WinHttp {
    pub fn new() -> Self {
        Self {
            agent: format!("GameModeExecutor/{}", crate::build_info::PACKAGE_VERSION),
            secure_only: true,
            proxy: WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            timeouts: TIMEOUTS_MS,
        }
    }

    /// For the tests alone: plain `http`, no proxy, and short timeouts so
    /// a listener that never answers is a fault in seconds.
    #[cfg(test)]
    fn plain() -> Self {
        Self {
            agent: "GameModeExecutor/test".to_owned(),
            secure_only: false,
            proxy: windows::Win32::Networking::WinHttp::WINHTTP_ACCESS_TYPE_NO_PROXY,
            timeouts: (2_000, 2_000, 2_000, 2_000),
        }
    }
}

impl Default for WinHttp {
    fn default() -> Self {
        Self::new()
    }
}

/// A WinHTTP handle closed on drop. Session, connection and request are all
/// the same kind of handle to the library.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle was returned by WinHTTP and is closed once.
            unsafe { _ = WinHttpCloseHandle(self.0) };
        }
    }
}

/// A URL split the way WinHTTP wants it.
#[derive(Debug, PartialEq, Eq)]
struct Target {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}

/// `https://host[:port]/path`, and `http://` only when the caller allows
/// it: a plain `http` release URL in the shipped program would be a
/// configuration mistake worth refusing.
fn split(url: &str, allow_plain: bool) -> Result<Target, Fault> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://").filter(|_| allow_plain) {
        (false, rest)
    } else {
        return Err(Fault::Unexpected(format!("not an https URL: {url}")));
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (
            host,
            port.parse::<u16>()
                .map_err(|_| Fault::Unexpected(format!("no port in {url}")))?,
        ),
        None => (
            authority,
            if secure {
                INTERNET_DEFAULT_HTTPS_PORT
            } else {
                INTERNET_DEFAULT_HTTP_PORT
            },
        ),
    };
    if host.is_empty() {
        return Err(Fault::Unexpected(format!("no host in {url}")));
    }
    Ok(Target {
        secure,
        host: host.to_owned(),
        port,
        path: format!("/{path}"),
    })
}

/// A WinHTTP error as a fault. The library's own codes are 12000 to 12175;
/// anything else is not a network problem and is said as it is.
fn fault(error: windows::core::Error, what: &str) -> Fault {
    let code = error.code().0 as u32 & 0xFFFF;
    tracing::debug!(
        target: crate::logging::target::UPDATE,
        code,
        error = %error,
        "{what} failed"
    );
    if (12000..=12200).contains(&code) {
        Fault::NoConnection { code }
    } else {
        Fault::Unexpected(format!("{what}: {error}"))
    }
}

/// A received response: its status, and the request handle to read from.
///
/// The session and the connection ride along, declared after the request
/// so they are closed after it: closing a parent handle cancels every
/// request under it, and a body read after that fails with 12017,
/// `ERROR_WINHTTP_OPERATION_CANCELLED` -- measured on the first real
/// download, 2026-09-18.
struct Response {
    request: Handle,
    status: u32,
    _connection: Handle,
    _session: Handle,
}

impl WinHttp {
    fn send(&self, verb: &str, url: &str, follow_redirects: bool) -> Result<Response, Fault> {
        let target = split(url, !self.secure_only)?;
        let agent = HSTRING::from(self.agent.as_str());
        // SAFETY: the agent string outlives the call; no proxy strings, the
        // system settings apply; the handle is owned by `Handle`.
        let session = Handle(unsafe {
            WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                self.proxy,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        });
        if session.0.is_null() {
            return Err(fault(windows::core::Error::from_thread(), "WinHttpOpen"));
        }
        // SAFETY: the session handle is open; the four values are
        // milliseconds.
        unsafe {
            WinHttpSetTimeouts(
                session.0,
                self.timeouts.0,
                self.timeouts.1,
                self.timeouts.2,
                self.timeouts.3,
            )
        }
        .map_err(|error| fault(error, "WinHttpSetTimeouts"))?;

        let host = HSTRING::from(target.host.as_str());
        // SAFETY: the session is open and the host string outlives the call.
        let connection =
            Handle(unsafe { WinHttpConnect(session.0, PCWSTR(host.as_ptr()), target.port, 0) });
        if connection.0.is_null() {
            return Err(fault(windows::core::Error::from_thread(), "WinHttpConnect"));
        }

        let verb_w = HSTRING::from(verb);
        let path = HSTRING::from(target.path.as_str());
        let flags = if target.secure {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        // SAFETY: the connection is open; the strings outlive the call; no
        // accept types (null-terminated list absent) and HTTP/1.1 by
        // default.
        let request = Handle(unsafe {
            WinHttpOpenRequest(
                connection.0,
                PCWSTR(verb_w.as_ptr()),
                PCWSTR(path.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flags,
            )
        });
        if request.0.is_null() {
            return Err(fault(
                windows::core::Error::from_thread(),
                "WinHttpOpenRequest",
            ));
        }
        if !follow_redirects {
            let policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes();
            // SAFETY: the request is open and the option value is a DWORD
            // passed as its bytes, as the option requires.
            unsafe {
                WinHttpSetOption(
                    Some(request.0),
                    WINHTTP_OPTION_REDIRECT_POLICY,
                    Some(&policy),
                )
            }
            .map_err(|error| fault(error, "WinHttpSetOption"))?;
        }
        // SAFETY: the request is open; no extra headers, no body.
        unsafe { WinHttpSendRequest(request.0, None, None, 0, 0, 0) }
            .map_err(|error| fault(error, "WinHttpSendRequest"))?;
        // SAFETY: the request was sent; the reserved argument is null.
        unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) }
            .map_err(|error| fault(error, "WinHttpReceiveResponse"))?;

        let status = query_number(&request, WINHTTP_QUERY_STATUS_CODE)
            .ok_or_else(|| Fault::Unexpected("no status code in the answer".to_owned()))?;
        tracing::debug!(
            target: crate::logging::target::UPDATE,
            verb,
            url,
            status,
            "Request answered"
        );
        Ok(Response {
            request,
            status,
            _connection: connection,
            _session: session,
        })
    }
}

fn query_number(request: &Handle, header: u32) -> Option<u32> {
    let mut value = 0u32;
    let mut length = std::mem::size_of::<u32>() as u32;
    let mut index = 0u32;
    // SAFETY: the request has received its response; `value` is the DWORD
    // the flag asks for, `length` its size, `index` a valid slot.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            header | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut value as *mut u32 as *mut c_void),
            &mut length,
            &mut index,
        )
    }
    .ok()
    .map(|()| value)
}

/// A text header of any length: asked for its size first, as the API
/// does it -- a signed asset URL runs to a kilobyte, and a fixed buffer
/// would have truncated a longer one in silence.
fn query_text(request: &Handle, header: u32) -> Option<String> {
    let mut length = 0u32;
    let mut index = 0u32;
    // SAFETY: no buffer, so the call only writes the byte length needed
    // into `length` and fails with ERROR_INSUFFICIENT_BUFFER -- or with
    // HEADER_NOT_FOUND, leaving `length` at zero.
    let _ = unsafe {
        WinHttpQueryHeaders(
            request.0,
            header,
            PCWSTR::null(),
            None,
            &mut length,
            &mut index,
        )
    };
    if length == 0 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize / 2 + 1];
    let mut index = 0u32;
    // SAFETY: the request has received its response; the buffer and its
    // byte length are what the call is given, and it writes at most that
    // much, NUL-terminated.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            header,
            PCWSTR::null(),
            Some(buffer.as_mut_ptr() as *mut c_void),
            &mut length,
            &mut index,
        )
    }
    .ok()?;
    let chars = (length as usize / 2).min(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..chars]))
}

/// Read the whole body through `sink`, in 64 KiB pieces.
fn read_body(
    request: &Handle,
    mut sink: impl FnMut(&[u8]) -> Result<(), Fault>,
) -> Result<u64, Fault> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let mut read = 0u32;
        // SAFETY: the request has received its response; the buffer and its
        // length are what the call may write, and `read` says how much it did.
        unsafe {
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr() as *mut c_void,
                buffer.len() as u32,
                &mut read,
            )
        }
        .map_err(|error| fault(error, "WinHttpReadData"))?;
        if read == 0 {
            return Ok(total);
        }
        sink(&buffer[..read as usize])?;
        total += u64::from(read);
    }
}

impl Feed for WinHttp {
    fn redirect_of(&self, url: &str) -> Result<String, Fault> {
        let response = self.send("HEAD", url, false)?;
        match response.status {
            301..=308 => query_text(&response.request, WINHTTP_QUERY_LOCATION)
                .ok_or_else(|| Fault::Unexpected(format!("{url} redirected without a Location"))),
            200 => Err(Fault::Unexpected(format!(
                "{url} answered a page instead of a redirect"
            ))),
            status => Err(Fault::Http { status }),
        }
    }

    fn text(&self, url: &str) -> Result<String, Fault> {
        let response = self.send("GET", url, true)?;
        if response.status != 200 {
            return Err(Fault::Http {
                status: response.status,
            });
        }
        let mut bytes = Vec::new();
        read_body(&response.request, |piece| {
            // A checksum file is a few hundred bytes; a megabyte here is a
            // page, not the file.
            if bytes.len() + piece.len() > 1024 * 1024 {
                return Err(Fault::Unexpected(format!(
                    "{url} answered more than a text file"
                )));
            }
            bytes.extend_from_slice(piece);
            Ok(())
        })?;
        String::from_utf8(bytes)
            .map_err(|_| Fault::Unexpected(format!("{url} did not answer text")))
    }

    fn download(
        &self,
        url: &str,
        to: &Path,
        progress: &mut dyn FnMut(Option<u64>),
    ) -> Result<u64, Fault> {
        let response = self.send("GET", url, true)?;
        if response.status != 200 {
            return Err(Fault::Http {
                status: response.status,
            });
        }
        let size = query_number(&response.request, WINHTTP_QUERY_CONTENT_LENGTH).map(u64::from);
        progress(size);
        let mut file = std::fs::File::create(to).map_err(|error| Fault::write(to, &error))?;
        let written = read_body(&response.request, |piece| {
            file.write_all(piece)
                .map_err(|error| Fault::write(to, &error))
        })?;
        file.flush().map_err(|error| Fault::write(to, &error))?;
        if let Some(size) = size
            && size != written
        {
            return Err(Fault::Unexpected(format!(
                "{url} announced {size} bytes and sent {written}"
            )));
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn only_https_urls_are_split_unless_the_tests_say_otherwise() {
        assert_eq!(
            split(
                "https://github.com/Geeooff/GameModeExecutor/releases/latest",
                false
            )
            .unwrap(),
            Target {
                secure: true,
                host: "github.com".to_owned(),
                port: 443,
                path: "/Geeooff/GameModeExecutor/releases/latest".to_owned()
            }
        );
        assert_eq!(split("https://github.com", false).unwrap().path, "/");
        assert!(matches!(
            split("http://github.com/x", false),
            Err(Fault::Unexpected(_))
        ));
        assert!(matches!(
            split("https:///x", false),
            Err(Fault::Unexpected(_))
        ));
        assert_eq!(
            split("http://127.0.0.1:8080/a", true).unwrap(),
            Target {
                secure: false,
                host: "127.0.0.1".to_owned(),
                port: 8080,
                path: "/a".to_owned()
            }
        );
        assert!(matches!(
            split("http://127.0.0.1:x/a", true),
            Err(Fault::Unexpected(_))
        ));
    }

    // ------------------------------------------------ a server of our own --

    /// What the listener answers to one `METHOD /path`: a status, headers,
    /// and a body that is sent unless the request was a `HEAD`.
    struct Answer {
        status: &'static str,
        headers: Vec<String>,
        body: Vec<u8>,
        /// Send only this many bytes of the body, then close: a cut-off
        /// download.
        truncate_to: Option<usize>,
    }

    fn redirect(to: String) -> Answer {
        Answer {
            status: "302 Found",
            headers: vec![format!("Location: {to}")],
            body: Vec::new(),
            truncate_to: None,
        }
    }

    fn ok(body: &[u8]) -> Answer {
        Answer {
            status: "200 OK",
            headers: Vec::new(),
            body: body.to_vec(),
            truncate_to: None,
        }
    }

    fn status(status: &'static str) -> Answer {
        Answer {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            truncate_to: None,
        }
    }

    /// An HTTP/1.1 listener on `127.0.0.1`, written by hand from the
    /// standard library: it reads one request's line and headers, answers
    /// from `routes` by `METHOD /path`, closes, and does that `count`
    /// times. What GitHub answers is in the design record's table; this is
    /// that table made to talk.
    fn serve(routes: Vec<(&'static str, Answer)>, count: usize) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for _ in 0..count {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut header = String::new();
                loop {
                    header.clear();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                }
                let mut parts = line.split_whitespace();
                let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let key = format!("{method} {path}");
                let mut stream = reader.into_inner();
                let answer = routes
                    .iter()
                    .find(|(route, _)| *route == key)
                    .map(|(_, answer)| answer);
                let Some(answer) = answer else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                };
                let mut head = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    answer.status,
                    answer.body.len()
                );
                for h in &answer.headers {
                    head.push_str(h);
                    head.push_str("\r\n");
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                if method != "HEAD" {
                    let sent = answer.truncate_to.unwrap_or(answer.body.len());
                    let _ = stream.write_all(&answer.body[..sent]);
                }
                let _ = stream.flush();
            }
        });
        port
    }

    fn scratch_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gme-winhttp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn the_check_reads_the_redirect_and_does_not_follow_it() {
        let port = serve(
            vec![(
                "HEAD /releases/latest",
                redirect("http://127.0.0.1:1/releases/tag/v0.2.0".to_owned()),
            )],
            1,
        );
        let feed = WinHttp::plain();
        assert_eq!(
            feed.redirect_of(&format!("http://127.0.0.1:{port}/releases/latest")),
            Ok("http://127.0.0.1:1/releases/tag/v0.2.0".to_owned()),
            "port 1 answers nothing, so following it would have failed"
        );
    }

    #[test]
    fn a_page_where_a_redirect_was_expected_is_an_unexpected_answer() {
        let port = serve(vec![("HEAD /releases/latest", ok(b"<html>"))], 1);
        let feed = WinHttp::plain();
        assert!(matches!(
            feed.redirect_of(&format!("http://127.0.0.1:{port}/releases/latest")),
            Err(Fault::Unexpected(_))
        ));
    }

    #[test]
    fn a_text_file_is_followed_to_another_host_and_read_whole() {
        // GitHub answers the checksum file through a redirect to another
        // host; here the other host is a second listener.
        let sums = b"0123abcd  GameModeExecutor-0.2.0.msi\n";
        let asset_host = serve(vec![("GET /blob/SHA256SUMS.txt", ok(sums))], 1);
        let port = serve(
            vec![(
                "GET /releases/download/v0.2.0/SHA256SUMS.txt",
                redirect(format!("http://127.0.0.1:{asset_host}/blob/SHA256SUMS.txt")),
            )],
            1,
        );
        let feed = WinHttp::plain();
        assert_eq!(
            feed.text(&format!(
                "http://127.0.0.1:{port}/releases/download/v0.2.0/SHA256SUMS.txt"
            )),
            Ok(String::from_utf8_lossy(sums).into_owned())
        );
    }

    #[test]
    fn a_download_is_written_whole_and_its_size_announced() {
        // Larger than one read, so the body arrives in pieces.
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
        let port = serve(vec![("GET /asset.msi", ok(&body))], 1);
        let feed = WinHttp::plain();
        let file = scratch_file("asset.msi");
        let mut announced = None;
        let written = feed
            .download(
                &format!("http://127.0.0.1:{port}/asset.msi"),
                &file,
                &mut |size| announced = size,
            )
            .unwrap();
        assert_eq!(written, body.len() as u64);
        assert_eq!(announced, Some(body.len() as u64));
        assert_eq!(std::fs::read(&file).unwrap(), body);
    }

    #[test]
    fn a_download_cut_short_is_an_unexpected_answer() {
        let body = vec![7u8; 10_000];
        let port = serve(
            vec![(
                "GET /asset.msi",
                Answer {
                    truncate_to: Some(4_000),
                    ..ok(&body)
                },
            )],
            1,
        );
        let feed = WinHttp::plain();
        let file = scratch_file("short.msi");
        let outcome = feed.download(
            &format!("http://127.0.0.1:{port}/asset.msi"),
            &file,
            &mut |_| {},
        );
        assert!(
            matches!(outcome, Err(Fault::Unexpected(ref text)) if text.contains("announced 10000 bytes and sent 4000")),
            "{outcome:?}"
        );
    }

    #[test]
    fn statuses_other_than_the_expected_one_are_http_faults() {
        let port = serve(
            vec![
                ("GET /gone", status("404 Not Found")),
                ("HEAD /down", status("503 Service Unavailable")),
            ],
            2,
        );
        let feed = WinHttp::plain();
        assert_eq!(
            feed.text(&format!("http://127.0.0.1:{port}/gone")),
            Err(Fault::Http { status: 404 })
        );
        assert_eq!(
            feed.redirect_of(&format!("http://127.0.0.1:{port}/down")),
            Err(Fault::Http { status: 503 })
        );
    }

    #[test]
    fn a_body_the_size_of_a_page_is_not_a_text_file() {
        let body = vec![b'x'; 1024 * 1024 + 1];
        let port = serve(vec![("GET /SHA256SUMS.txt", ok(&body))], 1);
        let feed = WinHttp::plain();
        assert!(matches!(
            feed.text(&format!("http://127.0.0.1:{port}/SHA256SUMS.txt")),
            Err(Fault::Unexpected(_))
        ));
    }

    #[test]
    fn a_port_nobody_listens_on_is_no_connection() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        // The listener is gone; the port is free again.
        let feed = WinHttp::plain();
        // ERROR_WINHTTP_CANNOT_CONNECT.
        assert_eq!(
            feed.redirect_of(&format!("http://127.0.0.1:{port}/releases/latest")),
            Err(Fault::NoConnection { code: 12029 })
        );
    }

    #[test]
    fn winhttp_errors_are_no_connection_and_others_are_said_as_they_are() {
        use windows::core::{Error, HRESULT};
        // ERROR_WINHTTP_NAME_NOT_RESOLVED, 12007, as an HRESULT.
        let dns = Error::from_hresult(HRESULT(0x80072EE7u32 as i32));
        assert_eq!(fault(dns, "send"), Fault::NoConnection { code: 12007 });
        // ERROR_WINHTTP_TIMEOUT, 12002.
        let timeout = Error::from_hresult(HRESULT(0x80072EE2u32 as i32));
        assert_eq!(fault(timeout, "send"), Fault::NoConnection { code: 12002 });
        // E_ACCESSDENIED is nobody's network.
        let denied = Error::from_hresult(HRESULT(0x80070005u32 as i32));
        assert!(
            matches!(fault(denied, "open"), Fault::Unexpected(text) if text.starts_with("open: "))
        );
    }
}
