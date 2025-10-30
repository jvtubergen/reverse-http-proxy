# reverse-http-proxy

_A minimal reverse proxy for path-based HTTP routing._

A lightweight, high-performance reverse proxy written in Rust. Routes HTTP requests to different backend servers based on URL paths using efficient bidirectional binary streaming with minimal overhead.

**Note:** URL path rewriting is available via the `--rewrite` flag (disabled by default). When disabled, the complete original path is forwarded unchanged to the backend server.

## Features

- **Path-based routing** - Route requests to different backends based on URL paths
- **Prefix matching** - Supports both exact and prefix-based path matching (longest match wins)
- **Optional path rewriting** - Strip matched route prefixes from forwarded requests
- **Binary streaming** - Forwards raw TCP bytes without parsing HTTP bodies
- **High performance** - Minimal overhead using tokio async I/O
- **Protocol agnostic** - Supports HTTP/1.1, HTTP/2, WebSockets, and Server-Sent Events
- **Default fallback** - Unmatched paths route to a default backend

## Quick Start

```bash
# Basic usage with default backend only
reverse-http-proxy 0.0.0.0:8080 127.0.0.1:3000

# With path-based routing
reverse-http-proxy 0.0.0.0:8080 127.0.0.1:3000 \
  -r /api=127.0.0.1:4000 \
  -r /webhook=127.0.0.1:5000
```

In this example:
- Proxy listens on `0.0.0.0:8080`
- Requests to `/api/*` go to `127.0.0.1:4000`
- Requests to `/webhook/*` go to `127.0.0.1:5000`
- All other requests go to the default backend `127.0.0.1:3000`

## Installation

You'll need Rust installed. Get it from [rustup.rs](https://rustup.rs/).

```bash
cd reverse-http-proxy
cargo build --release
```

The binary will be at `target/release/reverse-http-proxy`.

## Usage

### Basic Syntax

```bash
reverse-http-proxy <LISTEN_ADDRESS> <DEFAULT_BACKEND> [OPTIONS]
```

### Arguments

- `LISTEN_ADDRESS` - Address to listen on (format: `ip:port`)
- `DEFAULT_BACKEND` - Default backend address for unmatched paths (format: `ip:port`)

### Options

- `-r, --route <PATH=BACKEND>` - Add a path-based route (can be specified multiple times)
  - Format: `/path=ip:port`
  - Path must start with `/`
- `--rewrite` - Enable path rewriting (strips matched route prefix from forwarded requests)

### Examples

#### API Gateway pattern
```bash
reverse-http-proxy 0.0.0.0:80 127.0.0.1:3000 \
  -r /api/v1=127.0.0.1:4001 \
  -r /api/v2=127.0.0.1:4002 \
  -r /admin=127.0.0.1:5000 \
  -r /static=127.0.0.1:6000
```

#### Microservices routing
```bash
reverse-http-proxy 0.0.0.0:8080 127.0.0.1:3000 \
  -r /users=127.0.0.1:4001 \
  -r /orders=127.0.0.1:4002 \
  -r /payments=127.0.0.1:4003
```

#### Webhook fanout
```bash
reverse-http-proxy 0.0.0.0:8080 127.0.0.1:3000 \
  -r /webhook/github=127.0.0.1:5000 \
  -r /webhook/stripe=127.0.0.1:5001 \
  -r /webhook/slack=127.0.0.1:5002
```

## Routing Behavior

The proxy uses **longest prefix matching** for routing:

1. **Exact match** - If the path exactly matches a route, use that backend
2. **Prefix match** - If the path starts with a route prefix, use that backend
3. **Default fallback** - If no match, use the default backend

### Routing Examples

Given these routes:
```bash
-r /api=backend1:4000 \
-r /api/v2=backend2:5000 \
-r /webhook=backend3:6000
```

Request routing:
- `GET /` → default backend
- `GET /api` → `backend1:4000` (exact match)
- `GET /api/users` → `backend1:4000` (prefix match)
- `GET /api/v2/users` → `backend2:5000` (longest prefix match wins)
- `GET /webhook/stripe` → `backend3:6000` (prefix match)
- `GET /other` → default backend (no match)

### URL Path Rewriting

By default, the complete original path is forwarded to the backend server unchanged. You can enable path rewriting with the `--rewrite` flag to strip the matched route prefix.

#### Without path rewriting (default)

With route `-r /api=127.0.0.1:4000`:
- Client requests: `GET /api/users`
- Backend receives: `GET /api/users` (unchanged)

#### With path rewriting (`--rewrite`)

With route `-r /api=127.0.0.1:4000 --rewrite`:
- Client requests: `GET /api/users`
- Backend receives: `GET /users` (prefix stripped)

**Examples:**

```bash
# Without rewriting (default)
reverse-http-proxy 0.0.0.0:8080 127.0.0.1:3000 -r /api=127.0.0.1:4000
# Request to /api/test -> backend receives /api/test

# With rewriting
reverse-http-proxy 0.0.0.0:8080 127.0.0.1:3000 -r /api=127.0.0.1:4000 --rewrite
# Request to /api/test -> backend receives /test
```

**Path rewriting behavior:**
- Strips the matched route prefix from the request path
- Ensures the rewritten path always starts with `/`
- Works with both exact and prefix matches
- Only rewrites if a route matches (default backend requests are never rewritten)

## Architecture

The proxy operates in these key steps:

1. Accept incoming HTTP connections on the specified address
2. Parse HTTP request headers to extract the URL path
3. Match the path against configured routes (longest prefix match)
4. Forward the complete request to the appropriate backend server
5. Stream responses bidirectionally between client and backend using raw TCP bytes

By forwarding raw TCP bytes after initial routing, it achieves high performance while supporting any HTTP protocol version transparently.

## Error Handling

- **502 Bad Gateway** - Returned when the backend server is unreachable
- **Connection errors** - Logged to stderr
- **Parse errors** - Logged when HTTP request headers cannot be parsed
