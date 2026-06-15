use super::*;

fn fresh() -> (tempfile::TempDir, LocalStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LocalStore::open(dir.path().join("ls.redb")).expect("open");
    (dir, store)
}

#[test]
fn set_get_roundtrip() {
    let (_dir, store) = fresh();
    let twap = store.module("twap").unwrap();
    twap.set("k", b"v").unwrap();
    assert_eq!(twap.get("k").unwrap().as_deref(), Some(&b"v"[..]));
}

#[test]
fn namespaces_isolate_modules() {
    let (_dir, store) = fresh();
    let a = store.module("a").unwrap();
    let b = store.module("b").unwrap();
    a.set("k", b"from-a").unwrap();
    b.set("k", b"from-b").unwrap();
    assert_eq!(a.get("k").unwrap().as_deref(), Some(&b"from-a"[..]));
    assert_eq!(b.get("k").unwrap().as_deref(), Some(&b"from-b"[..]));
}

#[test]
fn delete_then_get_is_none() {
    let (_dir, store) = fresh();
    let twap = store.module("twap").unwrap();
    twap.set("k", b"v").unwrap();
    twap.delete("k").unwrap();
    assert!(twap.get("k").unwrap().is_none());
}

#[test]
fn list_keys_strips_namespace_prefix() {
    let (_dir, store) = fresh();
    let twap = store.module("twap").unwrap();
    twap.set("posted:1", b"x").unwrap();
    twap.set("posted:2", b"y").unwrap();
    twap.set("other", b"z").unwrap();
    let keys = twap.list_keys("posted:").unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().all(|k| k.starts_with("posted:")));
}

#[test]
fn rejects_empty_namespace() {
    let (_dir, store) = fresh();
    let err = store.module("").unwrap_err();
    assert!(matches!(err, StorageError::InvalidNamespace(_)));
}

#[test]
fn prefix_is_fixed_32_bytes() {
    let (_dir, store) = fresh();
    let short = store.module("a").unwrap();
    let long = store.module(&"a".repeat(300)).unwrap();
    assert_eq!(short.prefix.len(), PREFIX_LEN);
    assert_eq!(long.prefix.len(), PREFIX_LEN);
    // Different inputs produce different prefixes.
    assert_ne!(short.prefix, long.prefix);
}

#[test]
fn prefix_is_deterministic() {
    let (_dir, store) = fresh();
    let m1 = store.module("twap-monitor").unwrap();
    let m2 = store.module("twap-monitor").unwrap();
    assert_eq!(m1.prefix, m2.prefix);
}

#[test]
fn similar_names_differ() {
    // Verify that names that share a common prefix don't collide.
    let (_dir, store) = fresh();
    let pa = store.module("module-a").unwrap();
    let pb = store.module("module-b").unwrap();
    assert_ne!(pa.prefix, pb.prefix);
}

#[test]
fn module_handles_share_underlying_data() {
    // Two `ModuleStore` handles for the same name see the same data —
    // confirms cloning is just an Arc bump, not a fresh DB view.
    let (_dir, store) = fresh();
    let m1 = store.module("twap").unwrap();
    let m2 = store.module("twap").unwrap();
    m1.set("k", b"v").unwrap();
    assert_eq!(m2.get("k").unwrap().as_deref(), Some(&b"v"[..]));
}
