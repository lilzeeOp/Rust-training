use anyhow::{bail, Context, Result};

const MAX_HEADERS: usize = 64;

/// Parses an HTTP/1.x request header block and rebuilds it for proxy forwarding.
///
/// Injected headers (X-Forwarded-For, X-Request-ID, Via, Connection) are added
/// fresh from our values. Any client-supplied versions of these headers are stripped
/// to prevent spoofing.
///
/// Returns `(rewritten_header_bytes, method, path)`.
pub fn build_forwarded_request(
    buf: &[u8],
    len: usize,
    client_ip: &str,
    upstream_host: &str,
    request_id: &str,
) -> Result<(Vec<u8>, String, String)> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers);

    let status = req.parse(&buf[..len]).context("failed to parse HTTP request")?;
    if status.is_partial() {
        bail!("incomplete HTTP headers");
    }

    let method = req.method.context("missing HTTP method")?.to_string();
    let path = req.path.context("missing HTTP path")?.to_string();

    // Headers we control — strip any client-supplied versions to prevent spoofing
    let strip = ["host", "connection", "x-forwarded-for", "x-request-id", "via"];

    let mut out = Vec::with_capacity(len + 256);

    // 1. Request line (unchanged)
    out.extend_from_slice(method.as_bytes());
    out.push(b' ');
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(b" HTTP/1.1\r\n");

    // 2. Rewritten Host
    out.extend_from_slice(b"Host: ");
    out.extend_from_slice(upstream_host.as_bytes());
    out.extend_from_slice(b"\r\n");

    // 3. Original headers minus the ones we control
    for header in req.headers.iter() {
        if header.name.is_empty() {
            break;
        }
        if strip.iter().any(|s| header.name.eq_ignore_ascii_case(s)) {
            continue;
        }
        out.extend_from_slice(header.name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(header.value);
        out.extend_from_slice(b"\r\n");
    }

    // 4. Injected observability + proxy headers
    out.extend_from_slice(b"X-Forwarded-For: ");
    out.extend_from_slice(client_ip.as_bytes());
    out.extend_from_slice(b"\r\n");

    out.extend_from_slice(b"X-Request-ID: ");
    out.extend_from_slice(request_id.as_bytes());
    out.extend_from_slice(b"\r\n");

    out.extend_from_slice(b"Via: 1.1 rust-proxy\r\n");
    out.extend_from_slice(b"Connection: close\r\n");

    // 5. End of headers
    out.extend_from_slice(b"\r\n");

    Ok((out, method, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(method: &str, path: &str, headers: &[(&str, &str)]) -> Vec<u8> {
        let mut req = format!("{} {} HTTP/1.1\r\n", method, path);
        for (k, v) in headers {
            req.push_str(&format!("{}: {}\r\n", k, v));
        }
        req.push_str("\r\n");
        req.into_bytes()
    }

    fn parse(buf: &[u8]) -> (Vec<u8>, String, String) {
        build_forwarded_request(buf, buf.len(), "10.0.0.1", "127.0.0.1:9091", "test-req-id")
            .unwrap()
    }

    #[test]
    fn test_rewrites_host_header() {
        let req = make_request("GET", "/", &[("Host", "example.com")]);
        let (out, _, _) = parse(&req);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Host: 127.0.0.1:9091\r\n"), "missing rewritten Host");
        assert!(!s.contains("Host: example.com"), "original Host should be gone");
    }

    #[test]
    fn test_injects_x_forwarded_for() {
        let req = make_request("GET", "/", &[("Host", "example.com")]);
        let (out, _, _) = parse(&req);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("X-Forwarded-For: 10.0.0.1\r\n"));
    }

    #[test]
    fn test_drops_spoofed_x_forwarded_for() {
        let req = make_request("GET", "/", &[
            ("Host", "example.com"),
            ("X-Forwarded-For", "evil-spoof-ip"),
        ]);
        let (out, _, _) = parse(&req);
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches("X-Forwarded-For").count(), 1, "exactly one X-Forwarded-For");
        assert!(!s.contains("evil-spoof-ip"), "spoofed IP must be dropped");
    }

    #[test]
    fn test_injects_x_request_id() {
        let req = make_request("GET", "/api", &[("Host", "example.com")]);
        let (out, _, _) = parse(&req);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("X-Request-ID: test-req-id\r\n"));
    }

    #[test]
    fn test_injects_via_header() {
        let req = make_request("POST", "/submit", &[("Host", "example.com")]);
        let (out, _, _) = parse(&req);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Via: 1.1 rust-proxy\r\n"));
    }

    #[test]
    fn test_sets_connection_close_and_strips_keep_alive() {
        let req = make_request("GET", "/", &[
            ("Host", "example.com"),
            ("Connection", "keep-alive"),
        ]);
        let (out, _, _) = parse(&req);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Connection: close\r\n"), "must set Connection: close");
        assert!(!s.contains("keep-alive"), "must strip keep-alive");
    }

    #[test]
    fn test_preserves_other_headers() {
        let req = make_request("GET", "/api", &[
            ("Host", "example.com"),
            ("Content-Type", "application/json"),
            ("Authorization", "Bearer token123"),
        ]);
        let (out, _, _) = parse(&req);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Content-Type: application/json\r\n"));
        assert!(s.contains("Authorization: Bearer token123\r\n"));
    }

    #[test]
    fn test_returns_correct_method_and_path() {
        let req = make_request("POST", "/api/users", &[("Host", "example.com")]);
        let (_, method, path) = parse(&req);
        assert_eq!(method, "POST");
        assert_eq!(path, "/api/users");
    }
}
