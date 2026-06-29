use super::*;

fn fresh() -> (tempfile::TempDir, LocalStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LocalStore::open(dir.path().join("ls.redb")).expect("open");
    (dir, store)
}

#[test]
fn set_get_roundtrip() {
    let (_dir, store) = fresh();
    store.set("twap", "k", b"v").unwrap();
    assert_eq!(store.get("twap", "k").unwrap().as_deref(), Some(&b"v"[..]));
}

#[test]
fn namespaces_isolate_modules() {
    let (_dir, store) = fresh();
    store.set("a", "k", b"from-a").unwrap();
    store.set("b", "k", b"from-b").unwrap();
    assert_eq!(
        store.get("a", "k").unwrap().as_deref(),
        Some(&b"from-a"[..])
    );
    assert_eq!(
        store.get("b", "k").unwrap().as_deref(),
        Some(&b"from-b"[..])
    );
}

#[test]
fn delete_then_get_is_none() {
    let (_dir, store) = fresh();
    store.set("twap", "k", b"v").unwrap();
    store.delete("twap", "k").unwrap();
    assert!(store.get("twap", "k").unwrap().is_none());
}

#[test]
fn list_keys_strips_namespace_prefix() {
    let (_dir, store) = fresh();
    store.set("twap", "posted:1", b"x").unwrap();
    store.set("twap", "posted:2", b"y").unwrap();
    store.set("twap", "other", b"z").unwrap();
    let keys = store.list_keys("twap", "posted:").unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().all(|k| k.starts_with("posted:")));
}

#[test]
fn rejects_empty_namespace() {
    let (_dir, store) = fresh();
    let err = store.set("", "k", b"v").unwrap_err();
    assert!(matches!(err, StorageError::InvalidNamespace(_)));
}

#[test]
fn prefix_is_fixed_32_bytes() {
    let short = namespace_prefix("a").unwrap();
    let long = namespace_prefix(&"a".repeat(300)).unwrap();
    assert_eq!(short.len(), PREFIX_LEN);
    assert_eq!(long.len(), PREFIX_LEN);
    // Different inputs produce different prefixes.
    assert_ne!(short, long);
}

#[test]
fn prefix_is_deterministic() {
    let p1 = namespace_prefix("twap-monitor").unwrap();
    let p2 = namespace_prefix("twap-monitor").unwrap();
    assert_eq!(p1, p2);
}

#[test]
fn similar_names_differ() {
    // Verify that names that share a common prefix don't collide.
    let pa = namespace_prefix("module-a").unwrap();
    let pb = namespace_prefix("module-b").unwrap();
    assert_ne!(pa, pb);
}

// ---------------------------------------------------------------------------
// Concurrent access tests
// ---------------------------------------------------------------------------

#[test]
fn concurrent_writes_from_different_namespaces() {
    let (_dir, store) = fresh();

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let s = store.clone();
            std::thread::spawn(move || {
                let ns = format!("ns-{i}");
                for j in 0..100 {
                    let key = format!("key-{j}");
                    let val = format!("val-{i}-{j}").into_bytes();
                    s.set(&ns, &key, &val).unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    for i in 0..8 {
        let ns = format!("ns-{i}");
        for j in 0..100 {
            let key = format!("key-{j}");
            let expected = format!("val-{i}-{j}").into_bytes();
            assert_eq!(
                store.get(&ns, &key).unwrap().as_deref(),
                Some(expected.as_slice()),
            );
        }
    }
}

#[test]
fn concurrent_reads_during_writes() {
    let (_dir, store) = fresh();

    // Pre-populate namespace "rw" with 50 keys.
    for j in 0..50 {
        store
            .set("rw", &format!("k-{j}"), b"old")
            .unwrap();
    }

    let writer_store = store.clone();
    let writer = std::thread::spawn(move || {
        for j in 0..50 {
            writer_store
                .set("rw", &format!("k-{j}"), b"new")
                .unwrap();
        }
    });

    let readers: Vec<_> = (0..4)
        .map(|_| {
            let s = store.clone();
            std::thread::spawn(move || {
                for _ in 0..100 {
                    for j in 0..50 {
                        let val = s.get("rw", &format!("k-{j}")).unwrap();
                        let val = val.expect("key must exist");
                        assert!(
                            val == b"old" || val == b"new",
                            "unexpected value: {:?}",
                            val,
                        );
                    }
                }
            })
        })
        .collect();

    writer.join().expect("writer panicked");
    for r in readers {
        r.join().expect("reader panicked");
    }

    // Final state: all keys must be "new".
    for j in 0..50 {
        assert_eq!(
            store.get("rw", &format!("k-{j}")).unwrap().as_deref(),
            Some(&b"new"[..]),
        );
    }
}

#[test]
fn list_keys_races_with_delete() {
    let (_dir, store) = fresh();

    // Pre-populate namespace "race" with 100 keys.
    for i in 0..100 {
        store
            .set("race", &format!("k:{i}"), b"x")
            .unwrap();
    }

    let deleter_store = store.clone();
    let deleter = std::thread::spawn(move || {
        for i in 0..100 {
            deleter_store
                .delete("race", &format!("k:{i}"))
                .unwrap();
        }
    });

    let lister_store = store.clone();
    let lister = std::thread::spawn(move || {
        for _ in 0..50 {
            let keys = lister_store.list_keys("race", "k:").unwrap();
            assert!(
                keys.len() <= 100,
                "list_keys returned more keys than expected: {}",
                keys.len(),
            );
        }
    });

    deleter.join().expect("deleter panicked");
    lister.join().expect("lister panicked");
}

#[test]
fn stress_many_writers_one_namespace() {
    let (_dir, store) = fresh();

    let handles: Vec<_> = (0..8)
        .map(|i| {
            let s = store.clone();
            std::thread::spawn(move || {
                for j in 0..100 {
                    let key = format!("t{i}-k{j}");
                    let val = format!("v-{i}-{j}").into_bytes();
                    s.set("shared", &key, &val).unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    // Verify all 800 keys are present with correct values.
    for i in 0..8 {
        for j in 0..100 {
            let key = format!("t{i}-k{j}");
            let expected = format!("v-{i}-{j}").into_bytes();
            assert_eq!(
                store.get("shared", &key).unwrap().as_deref(),
                Some(expected.as_slice()),
            );
        }
    }
}
