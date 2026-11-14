use super::*;
use crate::ownership_handoff_model::{Authority, AuthorityUnit, Journal, Placement};
use crate::store::{ownership_journal, ownership_latch};

fn pinned_fixture(root: &Path, journal: &Path) -> (Owner, Authority) {
    let (store, mut route) = fixture(root, "a");
    // The isolated in-memory witness starts at its admitted genesis.
    route.generation = 0;
    let authority = Authority {
        store_id: route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        generation: route.generation,
        epoch: route.log_epoch.clone(),
        owner: Placement {
            principal: "fixture-owner".into(),
            native: [1; 32],
        },
        policy: route.policy.clone(),
        predecessor: None,
    };
    ownership_journal::create(
        journal,
        &Journal::Active {
            authority: authority.clone(),
        },
    )
    .unwrap();
    drop(store);
    // Traverse real S1 admission before claiming the owner lease; no test pin.
    let store =
        crate::store::ownership_admission_fixture::open(&root.join("a.db"), journal, &authority);
    let owner = Owner::prepare(
        catalog(vec![route.clone()]),
        root,
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    (owner, authority)
}

fn request(owner: &Owner) -> WriteRequest<'_> {
    let mut request = super::request(owner);
    request.owner_generation = Some(owner.route.generation);
    request
}

#[test]
fn pinned_owner_knot_holds_shared_lineage_latch_through_physical_commit() {
    use std::sync::atomic::AtomicBool;
    let temp = tempfile::tempdir().unwrap();
    let journal = temp.path().join("owner.journal");
    let (a, _) = pinned_fixture(temp.path(), &journal);
    let observed = Arc::new(AtomicBool::new(false));
    {
        let store = a.writer.lock();
        let path = a.placement.path.clone();
        let journal = journal.clone();
        let observed = observed.clone();
        *store
            .ownership_fence
            .as_ref()
            .unwrap()
            .after_sql
            .lock()
            .unwrap() = Some(Box::new(move || {
            let conn = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let facts: i64 = conn
                .query_row(
                    "SELECT count(*) FROM facts f JOIN terms t ON t.id=f.e WHERE t.iri='urn:test:subject' AND f.op=1 AND f.valid_to IS NULL",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(facts > 0, "callback preceded physical fact commit");
            assert!(
                ownership_latch::exclusive(&journal).is_err(),
                "lineage latch released before SQL completion scope"
            );
            observed.store(true, Ordering::Release);
        }));
    }
    let result = a.knot(&request(&a), &input(&a, "scope-qualified"));
    assert!(
        result.is_ok(),
        "pinned native owner did not enter shared commit scope: {result:?}"
    );
    assert!(
        observed.load(Ordering::Acquire),
        "actual P1 post-SQL hook was not reached"
    );
    assert!(ownership_latch::exclusive(&journal).is_ok());
    assert_eq!(values(&a)["rows"][0]["value"], "scope-qualified");
}

struct ScopedEmbeddingProvider {
    journal: PathBuf,
    active: Journal,
    retire: bool,
    called: Arc<std::sync::atomic::AtomicBool>,
}
impl crate::EmbeddingProvider for ScopedEmbeddingProvider {
    fn embed_text(&self, _: &str) -> Result<Vec<f32>> {
        assert!(
            ownership_latch::exclusive(&self.journal).is_ok(),
            "inference retained commit latch"
        );
        self.called.store(true, Ordering::Release);
        if self.retire {
            let Journal::Active { authority } = &self.active else {
                unreachable!()
            };
            let retiring = Journal::Retiring {
                request: crate::ownership_handoff_model::Request {
                    operation: "fixture-retirement".into(),
                    operator: "fixture-operator".into(),
                    source: authority.clone(),
                    successor_generation: authority.generation + 1,
                    destination: Placement {
                        principal: "fixture-next".into(),
                        native: [3; 32],
                    },
                    profile: "fixture-profile".into(),
                },
            };
            ownership_journal::advance(&self.journal, &self.active, &retiring)?;
        }
        Ok(vec![0.5; 8])
    }
    fn dimension(&self) -> usize {
        8
    }
}

#[test]
fn pinned_owner_embedding_reenters_same_scope_after_inference() {
    use crate::vector::KnowledgeVectorStore;
    let temp = tempfile::tempdir().unwrap();
    let journal = temp.path().join("embedding.journal");
    let (a, authority) = pinned_fixture(temp.path(), &journal);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut store = a.writer.lock();
        store.set_embedding_provider(Arc::new(ScopedEmbeddingProvider {
            journal: journal.clone(),
            active: Journal::Active { authority },
            retire: false,
            called: called.clone(),
        }));
        store.embedding_config_mut().auto_embed = true;
        store.search_config_mut().named_graphs = true;
        let path = a.placement.path.clone();
        let journal = journal.clone();
        let calls = calls.clone();
        *store
            .ownership_fence
            .as_ref()
            .unwrap()
            .after_sql
            .lock()
            .unwrap() = Some(Box::new(move || {
            let reader = Store::open_read_only(path.to_str().unwrap()).unwrap();
            let vectors = reader.vector_count().unwrap();
            let phase = calls.fetch_add(1, Ordering::AcqRel);
            assert_eq!(
                vectors, phase,
                "embedding scope did not cover physical vector commit"
            );
            assert!(ownership_latch::exclusive(&journal).is_err());
        }));
    }
    a.knot(&request(&a), &input(&a, "embedding-scoped"))
        .unwrap();
    assert!(
        called.load(Ordering::Acquire),
        "configured native provider not reached"
    );
    assert_eq!(
        calls.load(Ordering::Acquire),
        2,
        "facts and embedding need separate scopes"
    );
    assert_eq!(a.read(KnowledgeVectorStore::vector_count).unwrap(), 1);
    assert!(ownership_latch::exclusive(&journal).is_ok());
}

#[test]
fn retirement_during_inference_refuses_vector_write_without_retrying_fact_commit() {
    use crate::vector::KnowledgeVectorStore;
    let temp = tempfile::tempdir().unwrap();
    let journal = temp.path().join("retiring.journal");
    let (a, authority) = pinned_fixture(temp.path(), &journal);
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut store = a.writer.lock();
        store.set_embedding_provider(Arc::new(ScopedEmbeddingProvider {
            journal: journal.clone(),
            active: Journal::Active { authority },
            retire: true,
            called: called.clone(),
        }));
        store.embedding_config_mut().auto_embed = true;
        store.search_config_mut().named_graphs = true;
    }
    let result = a.knot(&request(&a), &input(&a, "facts-before-retirement"));
    assert!(
        called.load(Ordering::Acquire),
        "facts failed before actual inference"
    );
    assert!(
        result.is_err(),
        "retired generation applied stale embedding"
    );
    let reader = Store::open_read_only(a.placement.path.to_str().unwrap()).unwrap();
    assert_eq!(reader.vector_count().unwrap(), 0);
    assert!(
        reader
            .conn
            .query_row(
                "SELECT count(*) FROM facts WHERE op=1 AND valid_to IS NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap()
            > 0,
        "post-commit failure must not pretend earlier facts rolled back"
    );
    let fact_log_count = || {
        reader.conn.query_row(
        "SELECT count(*) FROM facts f JOIN terms t ON t.id=f.a WHERE t.iri='urn:test:value'",
        [], |r| r.get::<_, i64>(0),
    ).unwrap()
    };
    assert_eq!(
        fact_log_count(),
        1,
        "facts were retried after post-commit embedding refusal"
    );
    assert!(
        a.knot(&request(&a), &input(&a, "forbidden-later-write"))
            .is_err()
    );
    assert_eq!(
        fact_log_count(),
        1,
        "retired generation appended another fact"
    );
    assert_eq!(a.admitted.load(Ordering::Acquire), 0);
    assert!(ownership_latch::exclusive(&journal).is_ok());
}
