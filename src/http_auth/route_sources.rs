//! Test-only router source census, independent of server feature selection.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Hide explicit cfg(test) inline modules using their rustfmt item boundary.
/// A module's closing brace is at its declaration indentation; deeper braces
/// belong to its items. File modules are handled by collect instead.
fn production_source(source: &str) -> String {
    let lines: Vec<_> = source.lines().collect();
    let mut output = String::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim() == "#[cfg(test)]" {
            let mut item = at + 1;
            while item < lines.len()
                && (lines[item].trim().is_empty() || lines[item].trim().starts_with("#["))
            {
                item += 1;
            }
            if item < lines.len()
                && lines[item].trim().starts_with("mod ")
                && lines[item].trim().ends_with('{')
            {
                let indent = lines[item].len() - lines[item].trim_start().len();
                let mut end = item + 1;
                while end < lines.len()
                    && !(lines[end].trim() == "}"
                        && lines[end].len() - lines[end].trim_start().len() == indent)
                {
                    end += 1;
                }
                assert!(end < lines.len(), "test module has no source boundary");
                at = end + 1;
                continue;
            }
        }
        output.push_str(line);
        output.push('\n');
        at += 1;
    }
    output
}

/// Follow file-backed modules, including nested `#[path]` fragments. Read errors
/// and paths outside the source tree fail the census instead of hiding routes.
pub(super) fn collect(root: &Path) -> Vec<(PathBuf, String)> {
    let boundary = root
        .parent()
        .unwrap()
        .canonicalize()
        .expect("source directory");
    let mut pending = vec![(root.to_path_buf(), true)];
    let mut seen = BTreeSet::new();
    let mut sources = Vec::new();
    while let Some((file, crate_root)) = pending.pop() {
        let file = file.canonicalize().expect("router module must be readable");
        assert!(
            file.starts_with(&boundary),
            "router module outside source tree"
        );
        if !seen.insert(file.clone()) {
            continue;
        }
        let source =
            production_source(&std::fs::read_to_string(&file).expect("read router module"));
        let parent = file.parent().unwrap();
        let modules = if crate_root || file.file_name().unwrap() == "mod.rs" {
            parent.to_path_buf()
        } else {
            parent.join(file.file_stem().unwrap())
        };
        let mut explicit = None;
        let mut test_only = false;
        for line in source.lines() {
            let line = line.trim();
            if line == "#[cfg(test)]" {
                test_only = true;
                continue;
            }
            if let Some(attribute) = line.strip_prefix("#[path") {
                let value = attribute
                    .trim()
                    .strip_prefix('=')
                    .expect("path attribute equals")
                    .trim();
                let value = value.strip_prefix('"').expect("literal module path");
                let (path, tail) = value.split_once('"').expect("closed module path");
                assert_eq!(tail.trim(), "]", "unsupported path attribute");
                explicit = Some(parent.join(path));
                continue;
            }
            let declaration = if let Some(visible) = line.strip_prefix("pub(") {
                visible.split_once(')').expect("module visibility").1.trim()
            } else {
                line.strip_prefix("pub ").unwrap_or(line)
            };
            let declaration = declaration
                .split_once("//")
                .map_or(declaration, |(code, _)| code)
                .trim();
            let Some(name) = declaration
                .strip_prefix("mod ")
                .and_then(|s| s.strip_suffix(';'))
            else {
                if !line.is_empty() && !line.starts_with("#") && !line.starts_with("//") {
                    test_only = false;
                }
                continue;
            };
            // Only an explicit cfg(test) exempts a module, never its name.
            if std::mem::take(&mut test_only) {
                explicit = None;
                continue;
            }
            let target = explicit.take().unwrap_or_else(|| {
                let flat = modules.join(format!("{name}.rs"));
                let directory = modules.join(name).join("mod.rs");
                assert!(
                    !(flat.exists() && directory.exists()),
                    "ambiguous router module"
                );
                if flat.exists() { flat } else { directory }
            });
            pending.push((target, false));
        }
        assert!(explicit.is_none(), "path attribute without a file module");
        sources.push((file, source));
    }
    sources
}

/// Preserve the existing route/method census over all discovered modules.
pub(super) fn routes(
    sources: Vec<(PathBuf, String)>,
) -> (
    Vec<String>,
    std::collections::BTreeMap<String, BTreeSet<String>>,
) {
    let mut paths = Vec::new();
    let mut methods =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for (_, src) in sources {
        let mut remaining = src.as_str();
        while let Some(idx) = remaining.find(".route(") {
            remaining = &remaining[idx + ".route(".len()..];
            let candidate = remaining.trim_start();
            if let Some(path) = candidate.strip_prefix('"')
                && let Some(end) = path.find('"')
            {
                let name = path[..end].to_string();
                paths.push(name.clone());
                remaining = &path[end + 1..];
                let mut depth = 1usize;
                let mut end = 0;
                for (index, byte) in remaining.bytes().enumerate() {
                    match byte {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = index;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                assert!(end > 0, "route census lost declaration boundary for {name}");
                let body = &remaining[..end];
                let entry = methods.entry(name.clone()).or_default();
                for method in [
                    "get", "head", "post", "put", "patch", "delete", "options", "trace", "connect",
                    "any",
                ] {
                    if body.contains(&format!("{method}(")) {
                        entry.insert(method.to_string());
                    }
                }
                assert!(
                    !entry.is_empty(),
                    "unclassified method registration for {name}"
                );
            }
        }
    }
    (paths, methods)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "router module must be readable")]
    fn unreadable_source_cannot_be_an_empty_census() {
        collect(&std::env::temp_dir().join(format!("missing-router-{}.rs", std::process::id())));
    }

    #[test]
    fn inline_test_routers_do_not_hide_production_after_the_module() {
        let source = "#[cfg(test)]\nmod tests {\n    fn test() {\n        Router::new().route(\"/fake\", get(fake));\n    }\n}\nfn real() { Router::new().route(\"/real\", post(real)); }\n";
        let clean = production_source(source);
        let (paths, _) = routes(vec![(PathBuf::from("fixture.rs"), clean)]);
        assert_eq!(paths, ["/real"]);
    }

    #[test]
    fn recursively_finds_nested_path_and_normal_modules() {
        let dir = std::env::temp_dir().join(format!("route-census-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::create_dir(dir.join("server")).unwrap();
        std::fs::create_dir(dir.join("normal")).unwrap();
        std::fs::write(
            dir.join("server.rs"),
            "#[path = \"server/startup.rs\"]\nmod startup;\nmod normal;\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("server/startup.rs"),
            "#[path = \"read.rs\"]\nmod read;\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("server/read.rs"),
            "Router::new().route(\"/graph-sets/query\", post(read))",
        )
        .unwrap();
        std::fs::write(
            dir.join("normal.rs"),
            "mod child; // keep scanning\nmod test_router;\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("normal/child.rs"),
            "Router::new().route(\"/normal\", get(read))",
        )
        .unwrap();
        std::fs::write(
            dir.join("normal/test_router.rs"),
            "// production despite its name",
        )
        .unwrap();
        let sources = collect(&dir.join("server.rs"));
        assert_eq!(sources.len(), 6);
        assert!(sources.iter().any(|(_, s)| s.contains("/graph-sets/query")));
        assert!(sources.iter().any(|(_, s)| s.contains("/normal")));
        let (paths, methods) = routes(sources);
        assert_eq!(paths.len(), 2);
        assert_eq!(
            methods["/graph-sets/query"],
            BTreeSet::from(["post".into()])
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
