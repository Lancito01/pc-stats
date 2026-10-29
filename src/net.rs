// Sends one update over HTTPS with WinHTTP, so it uses Windows' own TLS and proxy settings.
// The app only ever makes outgoing requests: it never listens on a port or takes commands.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

use crate::util::wide;

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

// WinHTTP's error codes (12000 and up) have no system message text, so just show the number
fn last_error() -> u32 {
    unsafe { GetLastError() }
}

fn open(handle: *mut c_void, what: &str) -> Result<Handle, String> {
    if handle.is_null() {
        Err(format!("couldn't {what} (WinHTTP error {})", last_error()))
    } else {
        Ok(Handle(handle))
    }
}

/// Splits "https://host[:port]/path" into its parts. Plain http is refused.
pub fn parse(endpoint: &str) -> Option<(String, u16, String)> {
    let rest = endpoint.strip_prefix("https://")?;
    let (authority, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(a, p)| (a, format!("/{p}")));
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (authority, 443),
    };
    // Lowercase, like the Host header the site checks signatures against
    (!host.is_empty()).then(|| (host.to_ascii_lowercase(), port, path))
}

/// POSTs a signed update and returns the HTTP status code.
pub fn post(endpoint: &str, timestamp: u64, body: &str, signature: &str) -> Result<u32, String> {
    let (host, port, path) = parse(endpoint).ok_or("the endpoint isn't a valid https:// address")?;
    let agent = wide(concat!("pc-stats/", env!("CARGO_PKG_VERSION")));
    let headers = wide(&format!(
        "Content-Type: text/plain; charset=utf-8\r\nX-Stats-Timestamp: {timestamp}\r\nX-Stats-Signature: {signature}\r\n"
    ));

    unsafe {
        let session = open(
            WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0),
            "start WinHTTP",
        )?;
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 15_000);
        let connection = open(WinHttpConnect(session.0, wide(&host).as_ptr(), port, 0), "connect")?;
        let request = open(
            WinHttpOpenRequest(
                connection.0,
                wide("POST").as_ptr(),
                wide(&path).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "open the request",
        )?;

        let sent = WinHttpSendRequest(
            request.0,
            headers.as_ptr(),
            (headers.len() - 1) as u32,
            body.as_ptr() as *const c_void,
            body.len() as u32,
            body.len() as u32,
            0,
        ) != 0
            && WinHttpReceiveResponse(request.0, std::ptr::null_mut()) != 0;
        if !sent {
            return Err(format!("couldn't reach the site (WinHTTP error {})", last_error()));
        }

        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
        );
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_endpoints() {
        assert_eq!(
            parse("https://www.example.com/api/pc-stats/ingest"),
            Some(("www.example.com".into(), 443, "/api/pc-stats/ingest".into()))
        );
        assert_eq!(parse("https://localhost:8443/x"), Some(("localhost".into(), 8443, "/x".into())));
        assert_eq!(parse("https://Example.COM"), Some(("example.com".into(), 443, "/".into())));
        assert_eq!(parse("http://example.com/x"), None);
    }
}
