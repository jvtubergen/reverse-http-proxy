use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::net::SocketAddr;
use std::collections::HashMap;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "reverse-proxy")]
#[command(about = "Path-based reverse proxy with bidirectional binary streaming", long_about = None)]
struct Args {
    /// Address to listen on (format: ip:port)
    #[arg(value_name = "LISTEN_ADDRESS")]
    listen_address: String,

    /// Default backend address (format: ip:port)
    #[arg(value_name = "DEFAULT_BACKEND")]
    default_backend: String,

    /// Path-based routes in the format /path=ip:port (can be specified multiple times)
    #[arg(short = 'r', long = "route", value_name = "PATH=BACKEND")]
    routes: Vec<String>,
}

struct RouteConfig {
    default_backend: String,
    routes: HashMap<String, String>,
}

impl RouteConfig {
    fn new(default_backend: String, route_args: Vec<String>) -> Result<Self, String> {
        let mut routes = HashMap::new();

        for route in route_args {
            let parts: Vec<&str> = route.split('=').collect();
            if parts.len() != 2 {
                return Err(format!("Invalid route format: '{}'. Expected format: /path=ip:port", route));
            }

            let path = parts[0].to_string();
            let backend = parts[1].to_string();

            if !path.starts_with('/') {
                return Err(format!("Path must start with '/': {}", path));
            }

            routes.insert(path, backend);
        }

        Ok(RouteConfig {
            default_backend,
            routes,
        })
    }

    fn get_backend(&self, path: &str) -> &str {
        // Try exact match first
        if let Some(backend) = self.routes.get(path) {
            return backend;
        }

        // Try prefix matching (longest match wins)
        let mut best_match = "";
        let mut best_backend = &self.default_backend;

        for (route_path, backend) in &self.routes {
            if path.starts_with(route_path) && route_path.len() > best_match.len() {
                best_match = route_path;
                best_backend = backend;
            }
        }

        best_backend
    }
}

/// Parse the HTTP request to extract the path
/// Returns the path and the original request bytes
async fn parse_http_request(stream: &mut TcpStream) -> Result<(String, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
    let mut buffer = vec![0u8; 8192];
    let mut total_read = 0;

    // Read until we have the complete HTTP headers
    loop {
        let n = stream.read(&mut buffer[total_read..]).await?;
        if n == 0 {
            return Err("Connection closed before receiving complete headers".into());
        }

        total_read += n;

        // Check if we have the complete headers (look for \r\n\r\n)
        if let Some(pos) = find_header_end(&buffer[..total_read]) {
            // Parse just the headers to extract the path
            let headers_slice = &buffer[..pos];
            let mut headers = [httparse::EMPTY_HEADER; 64];
            let mut req = httparse::Request::new(&mut headers);

            match req.parse(headers_slice) {
                Ok(httparse::Status::Complete(_)) => {
                    let path = req.path.unwrap_or("/").to_string();

                    // Return the path and all the data we've read so far
                    let request_data = buffer[..total_read].to_vec();
                    return Ok((path, request_data));
                }
                Ok(httparse::Status::Partial) => {
                    // Need more data, continue reading
                    if total_read >= buffer.len() {
                        // Resize buffer if needed
                        buffer.resize(buffer.len() * 2, 0);
                    }
                    continue;
                }
                Err(e) => {
                    return Err(format!("Failed to parse HTTP request: {}", e).into());
                }
            }
        }

        // If buffer is full and we haven't found headers end, resize it
        if total_read >= buffer.len() {
            buffer.resize(buffer.len() * 2, 0);
        }
    }
}

/// Find the end of HTTP headers (\r\n\r\n)
fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i+4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    // Parse the routing configuration
    let config = RouteConfig::new(args.default_backend.clone(), args.routes)?;

    let addr = args.listen_address.parse::<SocketAddr>()?;
    let listener = TcpListener::bind(addr).await?;

    println!("Reverse proxy listening on http://{}", addr);
    println!("Default backend: http://{}", config.default_backend);

    if !config.routes.is_empty() {
        println!("\nPath-based routes:");
        for (path, backend) in &config.routes {
            println!("  {} -> http://{}", path, backend);
        }
    }

    let config = std::sync::Arc::new(config);

    loop {
        let (mut client_stream, client_addr) = listener.accept().await?;
        let config = config.clone();

        tokio::spawn(async move {
            // Parse the HTTP request to determine the path
            let (path, request_data) = match parse_http_request(&mut client_stream).await {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Failed to parse request from {}: {}", client_addr, e);
                    return;
                }
            };

            // Determine which backend to use based on the path
            let backend_addr = config.get_backend(&path);

            println!("[{}] {} -> {}", client_addr, path, backend_addr);

            // Connect to the backend server
            let mut backend_stream = match TcpStream::connect(backend_addr).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to connect to backend {}: {}", backend_addr, e);

                    // Send 502 Bad Gateway response
                    let response = b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 15\r\n\r\nBad Gateway\r\n";
                    let _ = client_stream.write_all(response).await;
                    return;
                }
            };

            // Forward the original request to the backend
            if let Err(e) = backend_stream.write_all(&request_data).await {
                eprintln!("Failed to forward request to backend: {}", e);
                return;
            }

            // Now do bidirectional streaming between client and backend
            if let Err(e) = tokio::io::copy_bidirectional(&mut client_stream, &mut backend_stream).await {
                // Connection errors are common and expected when clients/servers close connections
                if e.kind() != std::io::ErrorKind::UnexpectedEof
                    && e.kind() != std::io::ErrorKind::ConnectionReset {
                    eprintln!("Proxy forwarding error: {}", e);
                }
            }
        });
    }
}
