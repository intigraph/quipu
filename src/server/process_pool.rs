//! Opt-in local workers. Never retry a request or fall back to the writer.
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use quipu::{
    config::{QuipuConfig, VectorBackend},
    memory_budget::MemoryAllocation,
};
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;

const KEY: &str = "QUIPU_READ_WORKER_KEY";
const PARENT: &str = "QUIPU_READ_WORKER_PARENT";
const HEADER: &str = "x-quipu-read-worker-key";
const PID: &str = "x-quipu-read-worker-pid";
const PEER: &str = "x-quipu-read-worker-peer";

fn is_worker() -> bool {
    std::env::var_os(KEY).is_some()
}

pub(crate) fn validate(config: &QuipuConfig, args: &[String]) -> Result<(), String> {
    if config.server.read_processes == 0 {
        return Ok(());
    }
    if !cfg!(target_os = "linux") {
        return Err("reader processes currently require Linux parent-lifetime tracking".into());
    }
    if !config.attachments.is_empty()
        || !matches!(config.vector.backend, VectorBackend::Sqlite)
        || !config.federation.remotes.is_empty()
    {
        return Err("reader processes require local SQLite with no attachments or federation; refusing to drop providers".into());
    }
    if args.iter().any(|a| a == "--mcp-stdio") || config.store_path.to_str() == Some(":memory:") {
        return Err("reader processes require an on-disk store and an HTTP listener".into());
    }
    if config.server.reserve_point_reads {
        return Err("reader processes do not support reserve_point_reads".into());
    }
    if is_worker() {
        if config.server.replica.is_none() || std::env::var(PARENT).is_err() {
            return Err("incomplete reader worker identity".into());
        }
    } else if config.server.replica.is_some() || config.server.read_only {
        return Err("configure reader processes on the primary, not on an external replica/read_only server".into());
    }
    let bind: std::net::SocketAddr = config
        .server
        .bind
        .parse()
        .map_err(|_| "bind must be a numeric TCP address")?;
    if bind.port() == 0 {
        return Err("reader processes require a fixed primary port".into());
    }
    let end = u32::from(config.server.read_process_base_port) + config.server.read_processes as u32;
    if config.server.read_process_base_port == 0 || end > 65_536 {
        return Err("reader port range invalid".into());
    }
    if !is_worker()
        && (u32::from(config.server.read_process_base_port)..end).contains(&u32::from(bind.port()))
    {
        return Err("reader port range overlaps the primary listener".into());
    }
    Ok(())
}

fn parent_identity(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    if matches!(
        stat.rsplit_once(')')?.1.split_whitespace().next()?,
        "Z" | "X"
    ) {
        return None;
    }
    // Fields after comm begin at field3; starttime is field22. comm may contain spaces/parentheses.
    let start = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?;
    Some(format!("{pid}:{start}"))
}

struct Worker {
    url: String,
    pid: u32,
    identity: String,
    available: Arc<Semaphore>,
}

fn worker_input(address: &str) -> Result<Stdio, String> {
    #[cfg(target_os = "linux")]
    {
        let listener = std::net::TcpListener::bind(address).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(Stdio::from(std::os::fd::OwnedFd::from(listener)))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = address;
        Err("reader processes require Linux".into())
    }
}
struct Routing {
    configured: usize,
    workers: Vec<Worker>,
    key: String,
    next: AtomicUsize,
    timeout: Duration,
    response_limit: usize,
}

pub(crate) struct ProcessPool {
    children: Vec<Child>,
    routing: Option<Arc<Routing>>,
}
impl Drop for ProcessPool {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[path = "process_pool/startup.rs"]
mod startup;
pub(crate) use startup::start;

/// Only bounded data routes. Streams, exports, health/metrics and writes remain primary.
fn routed(path: &str, method: &str) -> bool {
    if !matches!(method, "GET" | "POST" | "HEAD")
        || quipu::http_auth::is_write_request(path, method)
    {
        return false;
    }
    matches!(
        path,
        "/query"
            | "/search"
            | "/search_query"
            | "/unified_search"
            | "/hybrid_search"
            | "/search_nodes"
            | "/search_facts"
            | "/search/nodes"
            | "/ask"
            | "/resolve"
            | "/graphs"
            | "/entity"
            | "/fragments"
            | "/reconcile"
    )
}
fn hop_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}
fn unavailable(message: &'static str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({"error":message,"reason":"read_process_unavailable"})),
    )
        .into_response()
}

async fn worker_guard(mut req: Request, next: Next) -> Response {
    let expected = std::env::var(KEY).unwrap_or_default();
    if expected.is_empty()
        || req.headers().get(HEADER).and_then(|v| v.to_str().ok()) != Some(expected.as_str())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Only the authenticated private channel may restore an original peer.
    // MCP calls without a socket peer stay UNKNOWN rather than becoming localhost.
    let peer = req
        .headers()
        .get(PEER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<std::net::SocketAddr>().ok());
    req.extensions_mut()
        .remove::<axum::extract::ConnectInfo<std::net::SocketAddr>>();
    if let Some(peer) = peer {
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(peer));
    }
    req.headers_mut().remove(PEER);
    req.headers_mut().remove(HEADER);
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        PID,
        HeaderValue::from_str(&std::process::id().to_string()).expect("numeric PID"),
    );
    response
}

pub(crate) fn route<S: Clone + Send + Sync + 'static>(
    app: Router<S>,
    pool: &ProcessPool,
) -> Router<S> {
    if let Some(routing) = &pool.routing {
        app.layer(middleware::from_fn_with_state(routing.clone(), proxy))
    } else {
        app
    }
}

pub(crate) fn protect_worker(app: Router) -> Router {
    if is_worker() {
        app.layer(middleware::from_fn(worker_guard))
    } else {
        app
    }
}

async fn proxy(State(routing): State<Arc<Routing>>, req: Request, next: Next) -> Response {
    if req.uri().path() == "/health" {
        let alive = routing
            .workers
            .iter()
            .filter(|worker| {
                parent_identity(worker.pid).as_deref() == Some(worker.identity.as_str())
            })
            .count();
        if alive != routing.configured {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({
                    "status": "degraded", "reason": "read_process_unavailable",
                    "read_processes": routing.configured, "read_processes_alive": alive
                })),
            )
                .into_response();
        }
        let mut response = next.run(req).await;
        response.headers_mut().insert(
            "x-quipu-read-processes-alive",
            HeaderValue::from_str(&alive.to_string()).expect("numeric count"),
        );
        return response;
    }
    if !routed(req.uri().path(), req.method().as_str()) {
        return next.run(req).await;
    }
    let start = routing.next.fetch_add(1, Ordering::Relaxed);
    let selected = (0..routing.workers.len()).find_map(|offset| {
        let worker = &routing.workers[(start.wrapping_add(offset)) % routing.workers.len()];
        worker
            .available
            .clone()
            .try_acquire_owned()
            .ok()
            .map(|permit| (worker, permit))
    });
    let Some((worker, permit)) = selected else {
        return unavailable("all read processes are busy");
    };
    let url = format!(
        "{}{}",
        worker.url,
        req.uri().path_and_query().map_or("/", |p| p.as_str())
    );
    let expected_pid = worker.pid.to_string();
    let peer = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.to_string());
    let (parts, body) = req.into_parts();
    let Ok(body) = to_bytes(body, 4 * 1024 * 1024).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit; // Client cancellation cannot free a still-running worker slot.
        let client = ureq::AgentBuilder::new()
            .timeout(routing.timeout)
            .redirects(0)
            .build();
        let mut request = client.request(parts.method.as_str(), &url);
        for (name, value) in &parts.headers {
            if !hop_header(name.as_str())
                && name.as_str() != HEADER
                && name.as_str() != PEER
                && name.as_str() != "accept-encoding"
                && let Ok(value) = value.to_str()
            {
                request = request.set(name.as_str(), value);
            }
        }
        request = request
            .set(HEADER, &routing.key)
            .set("accept-encoding", "identity");
        if let Some(peer) = peer {
            request = request.set(PEER, &peer);
        }
        let (Ok(response) | Err(ureq::Error::Status(_, response))) = request.send_bytes(&body)
        else {
            return unavailable("reader transport failed; request was not retried");
        };
        if response.header(PID) != Some(expected_pid.as_str()) {
            return unavailable("reader identity changed");
        }
        let status = response.status();
        let mut headers = axum::http::HeaderMap::new();
        for name in response.headers_names() {
            if !hop_header(&name)
                && name != HEADER
                && let (Ok(name), Some(value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    response.header(&name),
                )
                && let Ok(value) = HeaderValue::from_str(value)
            {
                headers.insert(name, value);
            }
        }
        let mut bytes = Vec::new();
        if response
            .into_reader()
            .take(routing.response_limit as u64 + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() > routing.response_limit
        {
            return unavailable("reader response exceeded its budget or was incomplete");
        }
        let mut result = Response::new(Body::from(bytes));
        *result.status_mut() = StatusCode::from_u16(status).expect("HTTP status");
        *result.headers_mut() = headers;
        result
    })
    .await
    {
        Ok(response) => response,
        Err(_) => unavailable("reader forwarding task failed"),
    }
}

#[cfg(test)]
#[path = "process_pool_tests.rs"]
mod tests;

/// Validate both resource and process topology before opening a store.
pub(crate) fn startup_allocation(
    config: &QuipuConfig,
    args: &[String],
) -> Option<quipu::memory_budget::MemoryAllocation> {
    let allocation = quipu::memory_budget::MemoryAllocation::for_server(&config.server)
        .unwrap_or_else(|error| {
            eprintln!("invalid memory budget: {error}");
            std::process::exit(1)
        });
    validate(config, args).unwrap_or_else(|error| {
        eprintln!("invalid read process configuration: {error}");
        std::process::exit(1)
    });
    allocation
}
