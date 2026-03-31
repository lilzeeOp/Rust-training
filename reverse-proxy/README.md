# Rust Reverse Proxy — 4 Options

A learning project building a production-grade reverse proxy in 4 progressive iterations, each adding a new layer of capability.

| Option | Crate | Port | What it adds |
|--------|-------|------|--------------|
| A | `a-tcp-passthrough` | 8080 | Raw TCP passthrough |
| B | `b-http-load-balancer` | 8081 | HTTP/1.1 parsing + round-robin load balancing |
| C | `c-tls-health-checks` | 8444 | TLS termination + background health checks |
| D | `d-http2-hyper` | 8443 | HTTP/2 via hyper + ALPN negotiation |

---

## Prerequisites

- [Rust](https://rustup.rs) (stable)
- `curl` (comes with Windows 11)
- Python 3 (for fake upstream servers — comes pre-installed on most systems)

Verify everything is ready:

```bash
rustc --version
curl --version
python --version
```

---

## Project structure

```
reverse-proxy/
├── Cargo.toml               # Workspace — all 4 crates share the build cache
├── a-tcp-passthrough/       # Option A
├── b-http-load-balancer/    # Option B
├── c-tls-health-checks/     # Option C
└── d-http2-hyper/           # Option D
```

Each crate has its own `config.toml` that controls the listen address, upstream addresses, TLS paths, and timeouts.

---

## Step 1 — Clone and build

```bash
git clone https://github.com/lilzeeOp/Rust-training.git
cd Rust-training/reverse-proxy
cargo build
```

Expected output (first build downloads dependencies, takes ~1 min):

```
   Compiling tokio v1.x.x
   ...
   Compiling d-http2-hyper v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 60s
```

---

## Step 2 — Start fake upstream servers

Each proxy forwards requests to a local backend. Use Python's built-in HTTP server as a stand-in.

Open **Terminal 1** in VSCode (`Ctrl+`` ` ```) and run:

```bash
python -m http.server 9090
```

Expected:
```
Serving HTTP on 0.0.0.0 port 9090 (http://0.0.0.0:9090/) ...
```

Open **Terminal 2** (`Ctrl+Shift+5` to split):

```bash
python -m http.server 9091
```

Expected:
```
Serving HTTP on 0.0.0.0 port 9091 (http://0.0.0.0:9091/) ...
```

Leave both terminals running for all four options.

---

## Option A — TCP Passthrough

**What it does:** Blindly pipes raw bytes between the client and a single upstream. No HTTP parsing, no header manipulation.

Open **Terminal 3** and run:

```bash
cd a-tcp-passthrough
cargo run
```

Expected startup output:
```
INFO a_tcp_passthrough: TCP proxy listening addr=127.0.0.1:8080 upstream=127.0.0.1:9090
```

Open **Terminal 4** and test:

```bash
curl http://localhost:8080/
```

Expected curl output (directory listing from the Python server):
```html
<!DOCTYPE HTML>
<html>...
<title>Directory listing for /</title>
...
```

Expected proxy log:
```
INFO [NEW]  client=127.0.0.1:54321 active=1
INFO [DONE] client=127.0.0.1:54321 active=0
```

Stop the proxy with `Ctrl+C` when done.

---

## Option B — HTTP/1.1 Load Balancer

**What it adds over A:** Parses HTTP/1.1 headers, rewrites them for proxy forwarding, and distributes requests across 3 upstreams using round-robin. Injects `X-Forwarded-For`, `X-Request-ID`, and `Via` headers.

Open **Terminal 3** and run:

```bash
cd b-http-load-balancer
cargo run
```

Expected startup output:
```
INFO b_http_load_balancer: HTTP proxy listening addr=127.0.0.1:8081 upstreams=3
```

Send 3 requests and watch the load balancer cycle through upstreams:

```bash
curl http://localhost:8081/
curl http://localhost:8081/
curl http://localhost:8081/
```

Expected proxy log (notice the upstream rotates):
```
INFO [a1b2c3d4] GET / → 127.0.0.1:9091 in 2ms
INFO [e5f6a7b8] GET / → 127.0.0.1:9092 in 1ms
INFO [c9d0e1f2] GET / → 127.0.0.1:9093 in 1ms
```

> Note: 9092 and 9093 are not running so those requests will return 502. Only 9091 is live. That is expected behaviour — the proxy is routing correctly, the backends just aren't all up.

Stop with `Ctrl+C`.

---

## Option C — HTTPS + TLS + Health Checks

**What it adds over B:** Terminates TLS (auto-generates a self-signed cert if none is provided), runs a background health-check loop per upstream, and skips unhealthy upstreams when load balancing. Returns 503 if all upstreams are down.

Open **Terminal 3** and run:

```bash
cd c-tls-health-checks
cargo run
```

Expected startup output:
```
INFO c_tls_health_checks: HTTPS proxy listening addr=0.0.0.0:8444 upstreams=2
```

Test with HTTPS (`-k` skips self-signed certificate verification):

```bash
curl -k https://localhost:8444/
```

Expected curl output: same HTML directory listing, now served over HTTPS.

Expected proxy log:
```
INFO [NEW]  client=127.0.0.1:54322 active=1
INFO [a1b2c3d4] GET / → 127.0.0.1:9091 in 3ms
INFO [DONE] client=127.0.0.1:54322 active=0
WARN upstream=127.0.0.1:9092 "health check FAILED"
```

The `health check FAILED` warning for 9092 is expected — you only started 9091. The proxy automatically skips 9092 and routes all traffic to 9091.

Stop with `Ctrl+C`.

---

## Option D — HTTPS + HTTP/2 via hyper

**What it adds over C:** Uses `hyper` as the HTTP engine (typed `Request`/`Response` instead of raw bytes), supports both HTTP/2 and HTTP/1.1 via ALPN negotiation on the TLS handshake, and enables true streaming of request/response bodies.

Open **Terminal 3** and run:

```bash
cd d-http2-hyper
cargo run
```

Expected startup output:
```
INFO d_http2_hyper: HTTPS/HTTP2 proxy listening addr=0.0.0.0:8443 upstreams=2
```

**Test with HTTP/1.1:**

```bash
curl -k https://localhost:8443/
```

**Test with HTTP/2** and see the protocol negotiated:

```bash
curl -k --http2 -v https://localhost:8443/ 2>&1 | findstr /i "alpn http/"
```

Expected output:
```
* ALPN: server accepted h2
< HTTP/2 200
```

This confirms the server accepted `h2` via ALPN and the response came back over HTTP/2.

Expected proxy log:
```
INFO [NEW]  client=127.0.0.1:54323 active=1
INFO [a1b2c3d4] GET / → 127.0.0.1:9091 in 4ms
INFO [DONE] client=127.0.0.1:54323 active=0
```

Stop with `Ctrl+C`.

---

## Run all tests

```bash
cargo test
```

Expected output:
```
test result: ok. 6 passed   # Option A
test result: ok. 18 passed  # Option B
test result: ok. 29 passed  # Option C
test result: ok. 20 passed  # Option D
```

73 tests total, 0 failures.

---

## What each option teaches

| Concept | A | B | C | D |
|---------|---|---|---|---|
| Async Rust / Tokio | Yes | Yes | Yes | Yes |
| Raw TCP I/O | Yes | Yes | Yes | Yes |
| HTTP/1.1 header parsing (`httparse`) | | Yes | Yes | |
| Round-robin load balancing | | Yes | Yes | Yes |
| TLS termination (`rustls`) | | | Yes | Yes |
| Background tasks / health checks | | | Yes | Yes |
| `AtomicBool` health flags (lock-free) | | | Yes | Yes |
| HTTP engine (`hyper`) | | | | Yes |
| HTTP/2 multiplexing | | | | Yes |
| ALPN protocol negotiation | | | | Yes |
| Streaming request/response bodies | | | | Yes |
| `Service` trait / typed handlers | | | | Yes |
