use std::io::Write;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// A null-terminated UTF-16 copy of `s`, for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Settings, state and the log live next to the exe, like obs-hud's.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn local_time() -> SYSTEMTIME {
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    now
}

/// The PC's local date, e.g. 2026-10-29. Daily counts reset when it changes.
pub fn local_day() -> String {
    let t = local_time();
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The URL-safe, unpadded form JWKs use.
pub fn base64url(data: &[u8]) -> String {
    base64(data).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// Escapes a string for a JSON string literal.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub mod log {
    use super::*;

    fn path() -> PathBuf {
        exe_dir().join("pc-stats.log")
    }

    pub fn init() {
        if std::fs::metadata(path()).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
            let _ = std::fs::remove_file(path());
        }
    }

    pub fn write(message: &str) {
        let t = local_time();
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path()) {
            let _ = writeln!(
                file,
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {}",
                t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, message
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn json_string_escapes() {
        assert_eq!(json_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(json_string("tab\there"), r#""tab\u0009here""#);
    }
}
