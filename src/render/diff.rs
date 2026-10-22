use super::publish::Stamp;
use super::{RenderPlan, invalid};
use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use cap_std::fs::Dir;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct Changes {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub noop: bool,
    pub diffs: Vec<FileDiff>,
}
#[derive(Debug, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub before_mode: Option<u32>,
    pub after_mode: Option<u32>,
    pub patch: Option<String>,
    pub patch_omitted: Option<String>,
}

pub(crate) fn differences(
    plan: &RenderPlan,
    old: &BTreeMap<String, Stamp>,
    root: Option<&Dir>,
    noop: bool,
) -> Result<Changes> {
    let new = &plan.files;
    let mut result = Changes {
        added: new
            .keys()
            .filter(|p| !old.contains_key(*p))
            .cloned()
            .collect(),
        changed: new
            .iter()
            .filter(|(p, s)| {
                old.get(*p)
                    .is_some_and(|o| o.sha256 != s.sha256 || o.mode != s.mode)
            })
            .map(|(p, _)| p.clone())
            .collect(),
        removed: old
            .keys()
            .filter(|p| !new.contains_key(*p))
            .cloned()
            .collect(),
        noop,
        diffs: Vec::new(),
    };
    let mut total_patch = 0;
    for path in result
        .added
        .iter()
        .chain(&result.changed)
        .chain(&result.removed)
    {
        let before = if old.contains_key(path) {
            root.expect("existing file has root")
                .read(path)
                .map_err(|e| invalid(e.to_string()))?
        } else {
            Vec::new()
        };
        let after = if let Some(file) = new.get(path) {
            STANDARD
                .decode(&file.base64)
                .map_err(|e| invalid(e.to_string()))?
        } else {
            Vec::new()
        };
        let mut patch = None;
        let mut omitted = None;
        if before != after {
            if before.len() + after.len() > 256 * 1024 || total_patch > 1024 * 1024 {
                omitted =
                    Some("text preview cap; hashes and membership diff remain complete".into());
            } else if let (Ok(before), Ok(after)) =
                (std::str::from_utf8(&before), std::str::from_utf8(&after))
            {
                let mut text = format!(
                    "--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
                    before.lines().count(),
                    after.lines().count()
                );
                for line in before.lines() {
                    text.push_str(&format!("-{line}\n"));
                }
                for line in after.lines() {
                    text.push_str(&format!("+{line}\n"));
                }
                total_patch += text.len();
                patch = Some(text);
            } else {
                omitted = Some("binary bytes; content hashes shown".into());
            }
        }
        result.diffs.push(FileDiff {
            path: path.clone(),
            before_sha256: old.get(path).map(|s| s.sha256.clone()),
            after_sha256: new.get(path).map(|s| s.sha256.clone()),
            before_mode: old.get(path).map(|s| s.mode),
            after_mode: new.get(path).map(|s| s.mode),
            patch,
            patch_omitted: omitted,
        });
    }
    Ok(result)
}
