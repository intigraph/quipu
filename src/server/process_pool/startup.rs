//! Bounded worker startup: retain primary service when one reader is unavailable.
use super::*;

pub(crate) fn start(config: &QuipuConfig, args: &[String]) -> Result<ProcessPool, String> {
    let mut pool = ProcessPool {
        children: Vec::new(),
        routing: None,
    };
    if config.server.read_processes == 0 {
        return Ok(pool);
    }
    if is_worker() {
        let expected = std::env::var(PARENT).map_err(|_| "missing worker parent")?;
        let pid = expected
            .split(':')
            .next()
            .and_then(|n| n.parse::<u32>().ok())
            .ok_or("invalid worker parent")?;
        if parent_identity(pid).as_deref() != Some(&expected) {
            return Err("worker parent is gone".into());
        }
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if parent_identity(pid).as_deref() != Some(&expected) {
                    std::process::exit(1);
                }
            }
        });
        return Ok(pool);
    }
    let allocation = MemoryAllocation::for_server(&config.server)
        .map_err(|e| e.to_string())?
        .ok_or("missing memory budget")?;
    let mut random = [0u8; 32];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| "worker identity randomness unavailable")?;
    let key: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = parent_identity(std::process::id()).ok_or("cannot establish parent start time")?;
    let public: std::net::SocketAddr = config
        .server
        .bind
        .parse()
        .map_err(|_| "invalid primary bind")?;
    let loopback = if public.is_ipv6() { "::1" } else { "127.0.0.1" };
    let primary = format!(
        "http://{}",
        std::net::SocketAddr::new(loopback.parse().expect("loopback literal"), public.port())
    );
    let db = std::fs::canonicalize(&config.store_path).map_err(|e| e.to_string())?;
    let mut workers = Vec::new();
    for index in 0..config.server.read_processes {
        let port = config.server.read_process_base_port + index as u16;
        let address = format!("127.0.0.1:{port}");
        // Reserve the actual listener, not a port number followed by a race.
        // No worker credential is ever sent to a pre-existing local listener.
        let input = match worker_input(&address) {
            Ok(input) => input,
            Err(error) => {
                eprintln!("reader worker {index} unavailable: {error}; primary retained");
                continue;
            }
        };
        let child = match Command::new(&executable)
            .args([
                "--db",
                db.to_str().ok_or("non-UTF8 database path")?,
                "--bind",
                &address,
                "--read-only",
                "--primary-url",
                &primary,
            ])
            .env(KEY, &key)
            .env(PARENT, &parent)
            .env("TOKIO_WORKER_THREADS", "2")
            .env("QUIPU_READ_WORKER_LISTENER_FD", "0")
            .stdin(input)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                eprintln!("reader worker {index} unavailable: {error}; primary retained");
                continue;
            }
        };
        let pid = child.id();
        pool.children.push(child);
        let url = format!("http://{address}");
        let mut ready = false;
        let client = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(200))
            .redirects(0)
            .build();
        for _ in 0..100 {
            match pool.children.last_mut().expect("child inserted").try_wait() {
                Ok(None) => {}
                Ok(Some(_)) | Err(_) => break,
            }
            if let Ok(response) = client
                .get(&format!("{url}/health"))
                .set(HEADER, &key)
                .set("X-Quipu-Client", "read-worker-startup")
                .call()
                && response.header(PID) == Some(pid.to_string().as_str())
            {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !ready {
            let mut child = pool.children.pop().expect("child inserted");
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("reader worker {index} failed startup; primary retained");
            continue;
        }
        let Some(identity) = parent_identity(pid) else {
            let mut child = pool.children.pop().expect("child inserted");
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("reader worker {index} exited during startup; primary retained");
            continue;
        };
        workers.push(Worker {
            url,
            pid,
            identity,
            available: Arc::new(Semaphore::new(1)),
        });
    }
    let timeout = Duration::from_millis(config.search.request_timeout_ms.clamp(1_000, 120_000));
    pool.routing = Some(Arc::new(Routing {
        configured: config.server.read_processes,
        workers,
        key,
        next: AtomicUsize::new(0),
        timeout,
        response_limit: usize::try_from(
            (allocation.total_bytes / 8 / allocation.connections).min(64 * 1024 * 1024),
        )
        .map_err(|_| "response allocation overflow")?,
    }));
    eprintln!(
        "read process pool: {} separate read-only workers; total memory budget {} bytes",
        pool.routing
            .as_ref()
            .expect("configured routing")
            .workers
            .len(),
        allocation.total_bytes
    );
    let _ = args; // Child configuration comes from the same cwd, never a copied credential file.
    Ok(pool)
}
