//! aegis-44r G2: the read-authorization route census must cover what the
//! server can actually be asked to do, not only the `.route("...")` literals.
//!
//! Two gaps the literal scan cannot see:
//! 1. MCP tools. `/mcp` is mounted with `nest_service` and merged AFTER the
//!    scoped-read and auth layers (`serve`), so the outer request never meets
//!    those guards. That is safe only because every tool call is re-dispatched
//!    through `oneshot` into the guarded application at the REST route that
//!    `endpoint()` names. So each tool's route must itself be classified.
//! 2. Non-`route` mounts (`nest`, `nest_service`, `route_service`, `fallback`).
//!    A new one would serve paths no census entry describes.
use super::*;
use crate::read_authorization::routes::{RouteClass, classify};

fn tool_route(name: &str) -> (Method, String) {
    let (method, path) = endpoint(name);
    let route = path.split('?').next().unwrap_or_default().to_owned();
    (method, route)
}

#[test]
fn every_mcp_tool_dispatches_to_a_classified_route() {
    let definitions = crate::tool_definitions();
    assert!(
        definitions.len() >= 20,
        "only {} MCP tools enumerated; refusing to pass on a registry that found almost nothing",
        definitions.len()
    );
    let mut unclassified = Vec::new();
    for definition in &definitions {
        let name = definition["name"].as_str().expect("manifest name");
        let (method, route) = tool_route(name);
        if classify(&route, method.as_str()) == RouteClass::Unclassified {
            unclassified.push(format!("{name} -> {method} {route}"));
        }
    }
    assert!(
        unclassified.is_empty(),
        "MCP tools dispatch to routes with no scoped-read classification: {unclassified:?}. \
         Classify the route (read_authorization/routes.rs), or fix endpoint()."
    );
}

#[test]
fn an_unknown_tool_name_falls_to_an_unclassified_route() {
    // Control: the default endpoint() mapping is POST /<name>. A new tool with no
    // matching route must be caught by the test above rather than slip through.
    let (method, route) = tool_route("quipu_census_probe_not_a_tool");
    assert_eq!(
        (method.as_str(), route.as_str()),
        ("POST", "/census_probe_not_a_tool")
    );
    assert_eq!(classify(&route, method.as_str()), RouteClass::Unclassified);
}

/// Remove only inline `#[cfg(test)] mod tests { ... }` blocks (brace-matched),
/// keeping any production code that follows them in the same file.
fn without_inline_tests(text: &str) -> String {
    const OPEN: &str = "\n#[cfg(test)]\nmod tests {";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let body = &rest[at + OPEN.len()..];
        let mut depth = 1usize;
        let mut end = body.len();
        for (index, byte) in body.bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = index + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &body[end..];
    }
    out.push_str(rest);
    out
}

#[test]
fn inline_test_removal_keeps_code_after_the_module() {
    let text = "a\n#[cfg(test)]\nmod tests {\n fn t() { x.nest(\"/t\"); }\n}\nb.nest(\"/after\")";
    let kept = without_inline_tests(text);
    assert!(!kept.contains("/t\""), "{kept}");
    assert!(kept.contains("b.nest(\"/after\")"), "{kept}");
}

/// Non-test Rust source under `src/`, with any trailing inline `mod tests`
/// removed, keyed by repo-relative path.
fn production_sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read src dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(&root.join("src"), &mut files);
    let mut sources = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(root)
            .expect("under manifest dir")
            .to_string_lossy()
            .replace('\\', "/");
        let file = rel.rsplit('/').next().unwrap_or_default();
        if file == "tests.rs" || file.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read source");
        sources.push((rel, without_inline_tests(&text)));
    }
    assert!(
        sources.len() > 100,
        "source walk found only {} files",
        sources.len()
    );
    sources
}

#[test]
fn non_route_mounts_and_out_of_census_routes_are_exactly_known() {
    let mut mounts = Vec::new();
    let mut outside = std::collections::BTreeSet::new();
    for (rel, text) in production_sources() {
        for call in [
            ".nest(",
            ".nest_service(",
            ".route_service(",
            ".fallback(",
            ".fallback_service(",
        ] {
            let mut rest = text.as_str();
            while let Some(at) = rest.find(call) {
                rest = &rest[at + call.len()..];
                let arg: String = rest
                    .chars()
                    .take_while(|c| *c != ',' && *c != ')')
                    .collect();
                mounts.push(format!("{rel} {call}{}", arg.trim()));
            }
        }
        let in_server = rel == "src/server.rs" || rel.starts_with("src/server/");
        if !in_server && text.contains(".route(") {
            outside.insert(rel);
        }
    }
    // /mcp: enforcement is per tool, through the guarded app (test above).
    assert_eq!(
        mounts,
        vec!["src/mcp_transport.rs .nest_service(\"/mcp\"".to_owned()],
        "a new non-route mount serves paths the read census does not describe; \
         classify what it serves and record how it is guarded"
    );
    // Route registrations outside src/server must be files the http_auth census
    // scans (http_auth/tests.rs `sources`); query_endpoint/routes.rs is one.
    assert_eq!(
        outside.into_iter().collect::<Vec<_>>(),
        vec!["src/query_endpoint/routes.rs".to_owned()],
        "routes registered outside src/server: add the file to the http_auth census sources"
    );
}
