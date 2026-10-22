use super::*;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn template_subset_refuses_intermediate_growth_and_capture() {
    let context = json!({"rows":[{"title":"alpha"}],"row":{"title":"alpha"}});
    assert_eq!(Markdown.render(Some("**{% for r in rows %}{% if r.title == 'alpha' %}{{ r.title|upper }}{% endif %}{% endfor %}** ~ literal"), &context).unwrap(), b"**ALPHA** ~ literal");
    for template in [
        "{% set s = 'x' %}{% set s = s ~ s %}{{ s[0] }}",
        "{{ 'x' * 1000000000 }}",
        "{{ '%}' ~ row.title }}",
        "{% filter upper %}{{ row.title }}{% endfilter %}",
        "{{ [rows, rows]|upper }}",
        "{% for r in rows recursive %}{{ loop(rows) }}{% endfor %}",
    ] {
        assert!(
            Markdown.render(Some(template), &context).is_err(),
            "{template}"
        );
    }
    assert!(Markdown.render(Some("{{ rows|upper }}"), &context).is_err());
}

#[test]
fn aggregate_cap_is_enforced_while_adding_files() {
    let mut files = BTreeMap::new();
    let half = vec![b'x'; MAX_BYTES / 2];
    plan::add_file(&mut files, "a".into(), &half, 0o644).unwrap();
    plan::add_file(&mut files, "b".into(), &half, 0o644).unwrap();
    assert!(plan::add_file(&mut files, "c".into(), b"x", 0o644).is_err());
    assert_eq!(files.len(), 2);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn publication_preserves_private_root_and_refuses_symlink_components() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("owned");
    let mut files = BTreeMap::new();
    plan::add_file(&mut files, "a".into(), b"one", 0o644).unwrap();
    let mut plan = RenderPlan {
        schema: 1,
        spec: "security".into(),
        source_tx: 0,
        valid_at: None,
        files,
    };
    publish::publish(&plan, &root, false).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    plan.source_tx = 1;
    publish::publish(&plan, &root, false).unwrap();
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(root.join(".quipu-render.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let outside = dir.path().join("outside");
    std::fs::write(&outside, b"retained").unwrap();
    assert!(publish::publish(&plan, &outside, false).is_err());
    symlink(dir.path(), dir.path().join("alias")).unwrap();
    assert!(publish::publish(&plan, &dir.path().join("alias/new"), false).is_err());
    symlink(&outside, root.join("link")).unwrap();
    assert!(publish::publish(&plan, &root, false).is_err());
    std::fs::remove_file(root.join("link")).unwrap();
    std::fs::hard_link(&outside, root.join("hard")).unwrap();
    assert!(publish::publish(&plan, &root, false).is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), b"retained");
}
