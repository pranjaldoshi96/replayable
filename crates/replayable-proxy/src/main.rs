//! Replayable L4 LLM API proxy entrypoint.
//!
//! v0.0.1 stub. Future work per docs/adr/0003-l4-proxy-language-and-design.md:
//! - hyper + tokio async HTTP server
//! - rustls TLS termination
//! - SSE streaming pass-through (zero buffering, preserve TTFT)
//! - Async trace export to ingest collector
//! - Sidecar deployment with Unix socket support

use replayable_proxy::version;

#[tokio::main]
async fn main() {
    println!("Replayable proxy v{}", version());
    println!("Status: stub — see docs/adr/0003-l4-proxy-language-and-design.md");
}
