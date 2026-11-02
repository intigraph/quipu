use super::*;
use crate::ownership_handoff_model::{
    Authority, AuthorityUnit, Certificate, Cut, Placement, Request,
};

fn placement(name: &str, byte: u8) -> Placement {
    Placement {
        principal: name.into(),
        native: [byte; 32],
    }
}

fn authority() -> Authority {
    Authority {
        store_id: "urn:example:lineage".into(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: "epoch-A".into(),
        owner: placement("A", 1),
        policy: "existing-policy-v1".into(),
        predecessor: None,
    }
}

fn request(source: Authority) -> Request {
    Request {
        operation: format!("op-B-{}", source.generation),
        operator: "existing-operator".into(),
        successor_generation: source.generation + 1,
        source,
        destination: placement("B", 2),
        profile: "existing-profile-v1".into(),
    }
}

fn certificate(request: Request) -> Certificate {
    Certificate {
        request,
        cut: Cut {
            transaction: 7,
            facts: [4; 32],
            artifact: [5; 32],
        },
    }
}

fn active() -> Journal {
    Journal::Active {
        authority: authority(),
    }
}

fn retiring() -> Journal {
    Journal::Retiring {
        request: request(authority()),
    }
}

fn retired() -> Journal {
    Journal::Retired {
        certificate: certificate(request(authority())),
    }
}

fn pending() -> (Journal, Authority) {
    let cert = certificate(request(authority()));
    let successor = cert.successor("epoch-B").unwrap();
    (
        Journal::Pending {
            certificate: cert,
            successor: successor.clone(),
        },
        successor,
    )
}

struct Dir {
    _tmp: tempfile::TempDir,
    path: PathBuf,
}

fn journal_path() -> Dir {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("lineage.journal");
    Dir { _tmp: tmp, path }
}

fn is_unknown(state: &JournalState) -> bool {
    matches!(state, JournalState::Unknown(_))
}

#[test]
fn a_created_journal_reads_back_and_cannot_be_recreated() {
    let dir = journal_path();
    create(&dir.path, &active()).unwrap();
    assert_eq!(read(&dir.path), JournalState::Known(Box::new(active())));
    assert!(create(&dir.path, &active()).is_err(), "no second creation");
    let other = journal_path();
    assert!(
        create(&other.path, &retiring()).is_err(),
        "a journal starts Active"
    );
    assert!(
        !sibling(&dir.path, ".staged").unwrap().exists(),
        "no staged file left behind"
    );
}

#[test]
fn anything_unreadable_is_unknown_never_legacy() {
    let dir = journal_path();
    assert!(is_unknown(&read(&dir.path)), "missing");

    std::fs::write(&dir.path, b"not a journal").unwrap();
    assert!(is_unknown(&read(&dir.path)), "no trailer");

    write_atomic(&dir.path, &active()).unwrap();
    let good = std::fs::read(&dir.path).unwrap();
    let mut flipped = good.clone();
    flipped[10] ^= 1;
    std::fs::write(&dir.path, &flipped).unwrap();
    assert!(is_unknown(&read(&dir.path)), "checksum mismatch");

    // A still-valid record under the OLD checksum: only the checksum catches it.
    let text = String::from_utf8(good.clone()).unwrap();
    let edited = text.replacen("epoch-A", "epoch-X", 1);
    assert_ne!(edited, text);
    std::fs::write(&dir.path, &edited).unwrap();
    match read(&dir.path) {
        JournalState::Unknown(why) => assert!(why.contains("checksum"), "{why}"),
        JournalState::Known(known) => panic!("an edited journal read as {known:?}"),
    }

    let (body, _) = text.rsplit_once(TRAILER).unwrap();
    std::fs::write(&dir.path, body).unwrap();
    assert!(is_unknown(&read(&dir.path)), "trailer cut off");

    std::fs::write(&dir.path, vec![b' '; MAX_RECORD_BYTES + 1024]).unwrap();
    assert!(is_unknown(&read(&dir.path)), "oversized");

    let with_checksum = |body: &str| {
        format!(
            "{body}{TRAILER}{}\n",
            hex::encode(Sha256::digest(body.as_bytes()))
        )
    };
    let newer = body.replacen("\"version\":1", "\"version\":2", 1);
    assert_ne!(newer, body);
    std::fs::write(&dir.path, with_checksum(&newer)).unwrap();
    assert!(is_unknown(&read(&dir.path)), "unknown version");

    let operation = Record {
        version: RECORD_VERSION,
        payload: Payload::Operation {
            request: request(authority()),
        },
    };
    let operation = String::from_utf8(operation.encode().unwrap()).unwrap();
    std::fs::write(&dir.path, with_checksum(&operation)).unwrap();
    match read(&dir.path) {
        JournalState::Unknown(why) => assert!(why.contains("not a journal"), "{why}"),
        JournalState::Known(known) => panic!("an operation record read as {known:?}"),
    }
}

#[test]
fn the_local_retirement_sequence_advances() {
    let dir = journal_path();
    create(&dir.path, &active()).unwrap();
    advance(&dir.path, &active(), &retiring()).unwrap();
    assert_eq!(read(&dir.path), JournalState::Known(Box::new(retiring())));
    advance(&dir.path, &retiring(), &retired()).unwrap();
    assert_eq!(read(&dir.path), JournalState::Known(Box::new(retired())));
}

#[test]
fn a_pending_destination_activates_only_its_successor() {
    let dir = journal_path();
    let (pending, successor) = pending();
    write_atomic(&dir.path, &pending).unwrap();
    let wrong = Authority {
        epoch: "epoch-C".into(),
        ..successor.clone()
    };
    assert!(advance(&dir.path, &pending, &Journal::Active { authority: wrong }).is_err());
    advance(
        &dir.path,
        &pending,
        &Journal::Active {
            authority: successor.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        read(&dir.path),
        JournalState::Known(Box::new(Journal::Active {
            authority: successor
        }))
    );
}

#[test]
fn illegal_or_stale_steps_refuse_and_leave_the_file_unchanged() {
    let other_source = Authority {
        epoch: "epoch-other".into(),
        ..authority()
    };
    let cases: Vec<(Journal, Journal, Journal)> = vec![
        // (stored, caller's expected, next)
        (
            active(),
            active(),
            Journal::Retiring {
                request: request(other_source.clone()),
            },
        ),
        (
            retiring(),
            retiring(),
            Journal::Retired {
                certificate: certificate(request(other_source)),
            },
        ),
        (retiring(), retiring(), active()),
        (retired(), retired(), active()),
        (retired(), retired(), retiring()),
        (active(), active(), retired()),
        (retiring(), active(), retired()),
    ];
    for (stored, expected, next) in cases {
        let dir = journal_path();
        write_atomic(&dir.path, &stored).unwrap();
        let before = std::fs::read(&dir.path).unwrap();
        assert!(
            advance(&dir.path, &expected, &next).is_err(),
            "{stored:?} -> {next:?} was admitted"
        );
        assert_eq!(
            std::fs::read(&dir.path).unwrap(),
            before,
            "a refusal changed the file"
        );
    }
}

#[test]
fn an_unknown_current_role_refuses_every_advance() {
    let dir = journal_path();
    std::fs::write(&dir.path, b"corrupt").unwrap();
    let err = advance(&dir.path, &active(), &retiring())
        .unwrap_err()
        .to_string();
    assert!(err.contains("UNKNOWN"), "{err}");
    assert_eq!(std::fs::read(&dir.path).unwrap(), b"corrupt");
}
