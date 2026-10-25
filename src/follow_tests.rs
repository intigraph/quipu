//! A follower driven from an in-process source: the fake feed serves exactly
//! what `GET /changes` and `GET /changes/stream` serve, so these cover the
//! bootstrap, the tail, restart, and every way the run must stop or reconnect.
use std::cell::RefCell;
use std::io::{BufRead, Cursor, Read};
use std::rc::Rc;

use super::*;
use crate::applier::live_digest;
use crate::store::Datum;
use crate::store::changes::Capture;
use crate::types::{Op, Value};

const T: &str = "2026-10-25T00:00:00Z";

fn assert_fact(store: &mut Store, entity: &str, value: &str, op: Op) {
    let datum = Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern("urn:p:title").unwrap(),
        value: Value::Str(value.into()),
        valid_from: T.into(),
        valid_to: None,
        op,
    };
    let g = store.graph_create("urn:graph:board").unwrap();
    store.transact_to_graph(&[datum], T, None, None, g).unwrap();
}

fn source_with(n: usize) -> Rc<RefCell<Store>> {
    let mut s = Store::open_in_memory().unwrap();
    for i in 0..n {
        assert_fact(
            &mut s,
            &format!("urn:item:{i}"),
            &format!("v{i}"),
            Op::Assert,
        );
    }
    Rc::new(RefCell::new(s))
}

/// `(route, since)` of every request, in order.
type Requests = Rc<RefCell<Vec<(&'static str, i64)>>>;

/// What the primary serves, built from a live source store.
struct Fake {
    source: Rc<RefCell<Store>>,
    requests: Requests,
    /// Cut the stream after this many frames with a read error.
    break_after: Option<usize>,
    /// A page of more transactions than this is over the page cap.
    max_page_txs: Option<usize>,
}

fn page_json(source: &Store, since: i64, limit: usize) -> String {
    let page = source
        .changes_after(since, limit, Capture::OldAndNewValues, None)
        .unwrap();
    page.to_json().to_string()
}

/// A reader that fails after its bytes run out, like a dropped connection.
struct Broken(Cursor<Vec<u8>>);
impl Read for Broken {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.read(buf)? {
            0 => Err(std::io::Error::other("connection reset")),
            n => Ok(n),
        }
    }
}

impl Feed for Fake {
    fn page(&mut self, since: i64, limit: usize) -> std::io::Result<String> {
        self.requests.borrow_mut().push(("page", since));
        if self.max_page_txs.is_some_and(|max| limit > max) {
            return Err(std::io::Error::other(PageTooLarge));
        }
        Ok(page_json(&self.source.borrow(), since, limit))
    }

    fn stream(&mut self, since: i64) -> std::io::Result<Box<dyn BufRead>> {
        self.requests.borrow_mut().push(("stream", since));
        let source = self.source.borrow();
        // One transaction per frame, as the server sends them, plus heartbeats.
        let mut text = String::from(": heartbeat\n\n");
        let (mut cursor, mut frames) = (since, 0);
        loop {
            let data = page_json(&source, cursor, 1);
            let page: serde_json::Value = serde_json::from_str(&data).unwrap();
            if page["records"].as_array().is_none_or(Vec::is_empty) {
                break;
            }
            if self.break_after == Some(frames) {
                let mut bytes = text.into_bytes();
                bytes.extend_from_slice(b"event: quipu.changes\ndata: {\"trunc");
                return Ok(Box::new(std::io::BufReader::new(Broken(Cursor::new(
                    bytes,
                )))));
            }
            let next = page["next_tx"].as_i64().unwrap();
            text.push_str(&format!(
                "event: quipu.changes\nid: {next}\ndata: {data}\n\n"
            ));
            text.push_str(": heartbeat\n\n");
            cursor = next;
            frames += 1;
        }
        Ok(Box::new(Cursor::new(text.into_bytes())))
    }
}

fn fake(source: &Rc<RefCell<Store>>) -> (Fake, Requests) {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let feed = Fake {
        source: source.clone(),
        requests: requests.clone(),
        break_after: None,
        max_page_txs: None,
    };
    (feed, requests)
}

fn small_pace() -> Pace {
    Pace {
        page_txs: 3,
        page_gap_ms: 7,
    }
}

#[test]
fn a_new_follower_bootstraps_in_paced_pages_and_equals_the_source() {
    let source = source_with(10);
    let mut replica = Store::open_in_memory().unwrap();
    let (feed, requests) = fake(&source);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    let mut slept = Vec::new();
    let pages = follower
        .bootstrap(small_pace(), &mut |ms| slept.push(ms))
        .unwrap();
    // 10 transactions at 3 per page: 4 pages, a pause between each.
    assert_eq!(pages, 4);
    assert_eq!(slept, vec![7, 7, 7]);
    assert_eq!(requests.borrow()[0], ("page", 0));
    let head = source.borrow().latest_tx_id().unwrap();
    assert_eq!(follower.cursor(), head);
    drop(follower);
    assert_eq!(live_digest(&replica), live_digest(&source.borrow()));
}

#[test]
fn the_tail_applies_new_commits_including_a_retract() {
    let source = source_with(2);
    let mut replica = Store::open_in_memory().unwrap();
    let (feed, _) = fake(&source);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    follower.bootstrap(small_pace(), &mut |_| {}).unwrap();
    {
        let mut s = source.borrow_mut();
        assert_fact(&mut s, "urn:item:new", "fresh", Op::Assert);
        assert_fact(&mut s, "urn:item:0", "v0", Op::Retract);
    }
    follower.tail_once(None).unwrap();
    assert_eq!(follower.cursor(), source.borrow().latest_tx_id().unwrap());
    drop(follower);
    let digest = live_digest(&replica);
    assert_eq!(digest, live_digest(&source.borrow()));
    assert!(
        !digest.iter().any(|f| f.contains("urn:item:0 ")),
        "{digest:?}"
    );
}

#[test]
fn a_restarted_follower_resumes_from_its_durable_cursor() {
    let source = source_with(4);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("follower.db");
    let path = path.to_str().unwrap();
    {
        let mut replica = Store::open(path).unwrap();
        let (feed, _) = fake(&source);
        let mut follower = Follower::open(&mut replica, feed).unwrap();
        follower.bootstrap(small_pace(), &mut |_| {}).unwrap();
    }
    assert_fact(
        &mut source.borrow_mut(),
        "urn:item:late",
        "late",
        Op::Assert,
    );
    let mut replica = Store::open(path).unwrap();
    let (feed, requests) = fake(&source);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    let before = follower.cursor();
    assert!(before > 0);
    follower.tail_once(None).unwrap();
    // The stream was asked for the transactions after the persisted cursor.
    assert_eq!(requests.borrow()[0], ("stream", before));
    drop(follower);
    assert_eq!(live_digest(&replica), live_digest(&source.borrow()));
}

#[test]
fn a_different_source_stops_the_follower_instead_of_splicing() {
    let source = source_with(3);
    let mut replica = Store::open_in_memory().unwrap();
    let (feed, _) = fake(&source);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    follower.bootstrap(small_pace(), &mut |_| {}).unwrap();
    let held = follower.cursor();
    drop(follower);
    // A restore or clone: another lineage behind the same URL.
    let other = source_with(5);
    let (feed, _) = fake(&other);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    let halt = follower.tail_once(None).unwrap_err();
    assert!(matches!(halt, Halt::Rebuild(_)), "{halt:?}");
    assert_eq!(follower.cursor(), held);
    drop(follower);
    assert_eq!(live_digest(&replica), live_digest(&source.borrow()));
}

#[test]
fn a_dropped_connection_keeps_every_whole_transaction_and_reconnects() {
    let source = source_with(1);
    let mut replica = Store::open_in_memory().unwrap();
    let (feed, _) = fake(&source);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    follower.bootstrap(small_pace(), &mut |_| {}).unwrap();
    for i in 0..3 {
        assert_fact(
            &mut source.borrow_mut(),
            &format!("urn:item:t{i}"),
            "t",
            Op::Assert,
        );
    }
    let start = follower.cursor();
    follower.feed.break_after = Some(2);
    let halt = follower.tail_once(None).unwrap_err();
    assert!(matches!(halt, Halt::Transport(_)), "{halt:?}");
    // Two whole transactions landed before the cut; the third did not.
    assert_eq!(follower.cursor(), start + 2);
    follower.feed.break_after = None;
    follower.tail_once(None).unwrap();
    drop(follower);
    assert_eq!(live_digest(&replica), live_digest(&source.borrow()));
}

#[test]
fn the_frame_budget_ends_a_tail_early() {
    let source = source_with(1);
    let mut replica = Store::open_in_memory().unwrap();
    let (feed, _) = fake(&source);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    follower.bootstrap(small_pace(), &mut |_| {}).unwrap();
    for i in 0..3 {
        assert_fact(
            &mut source.borrow_mut(),
            &format!("urn:item:b{i}"),
            "b",
            Op::Assert,
        );
    }
    let start = follower.cursor();
    let halt = follower.tail_once(Some(1)).unwrap_err();
    assert!(matches!(halt, Halt::Budget), "{halt:?}");
    assert_eq!(follower.cursor(), start + 1);
}

#[test]
fn sse_events_join_data_lines_and_skip_comments_and_ids() {
    let text =
        ": hb\r\n\r\nevent: a\r\nid: 7\r\ndata: one\r\ndata: two\r\n\r\ndata: plain\n\n: tail\n";
    let events: Vec<_> = SseEvents::new(Cursor::new(text))
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        events,
        vec![
            ("a".to_string(), "one\ntwo".to_string()),
            ("message".to_string(), "plain".to_string()),
        ]
    );
}

#[test]
fn an_oversized_page_is_asked_again_smaller_instead_of_retried_forever() {
    // grant's #843 finding: a page over the transport cap was a transport
    // failure, retried with the same cursor and limit forever.
    let source = source_with(10);
    let mut replica = Store::open_in_memory().unwrap();
    let (mut feed, _) = fake(&source);
    feed.max_page_txs = Some(1);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    let pace = Pace {
        page_txs: 8,
        page_gap_ms: 7,
    };
    follower.bootstrap(pace, &mut |_| ()).unwrap();
    assert_eq!(follower.cursor(), source.borrow().latest_tx_id().unwrap());
    drop(follower);
    assert_eq!(live_digest(&replica), live_digest(&source.borrow()));
}

#[test]
fn one_transaction_over_the_page_cap_stops_the_run() {
    let source = source_with(2);
    let mut replica = Store::open_in_memory().unwrap();
    let (mut feed, requests) = fake(&source);
    feed.max_page_txs = Some(0);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    let halt = follower.bootstrap(small_pace(), &mut |_| ()).unwrap_err();
    assert!(
        matches!(&halt, Halt::TooLarge(r) if r.contains("after 0")),
        "{halt:?}"
    );
    // 3 -> 2 -> 1 transactions, then a stop: never the same request forever.
    assert_eq!(requests.borrow().len(), 3);
    assert_eq!(follower.cursor(), 0);
}

#[test]
fn a_follower_is_ready_only_after_its_bootstrap_reaches_the_watermark() {
    use crate::follow_ready::{Readiness, readiness};
    let source = source_with(10);
    let expected = source.borrow().store_id().unwrap();
    let mut replica = Store::open_in_memory().unwrap();
    let (feed, _) = fake(&source);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    // Part of the history, as an interrupted bootstrap leaves it.
    let first = page_json(&source.borrow(), 0, 3);
    assert!(matches!(
        follower.apply("quipu.changes", &first),
        Ok(Step::Continue)
    ));
    assert_eq!(follower.cursor(), 3);
    let partial = readiness(follower.store, Some(&expected)).unwrap();
    assert!(
        matches!(&partial, Readiness::NotReady { reason } if reason.contains("bootstrap")),
        "{partial:?}"
    );
    follower.bootstrap(small_pace(), &mut |_| {}).unwrap();
    let head = source.borrow().latest_tx_id().unwrap();
    drop(follower);
    match readiness(&replica, Some(&expected)).unwrap() {
        Readiness::Ready {
            lineage, source_tx, ..
        } => {
            assert_eq!(lineage, expected);
            assert_eq!(source_tx, head);
        }
        other @ Readiness::NotReady { .. } => {
            panic!("a caught-up follower must be ready: {other:?}")
        }
    }
    // The right data for another store is still not this follower's job.
    let other = readiness(&replica, Some("urn:uuid:another-store")).unwrap();
    assert!(matches!(other, Readiness::NotReady { .. }), "{other:?}");
}

#[test]
fn a_follower_whose_source_changed_is_not_ready() {
    use crate::follow_ready::{Readiness, readiness};
    let source = source_with(3);
    let expected = source.borrow().store_id().unwrap();
    let mut replica = Store::open_in_memory().unwrap();
    let (feed, _) = fake(&source);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    follower.bootstrap(small_pace(), &mut |_| {}).unwrap();
    drop(follower);
    assert!(matches!(
        readiness(&replica, Some(&expected)).unwrap(),
        Readiness::Ready { .. }
    ));
    let other = source_with(5);
    let (feed, _) = fake(&other);
    let mut follower = Follower::open(&mut replica, feed).unwrap();
    assert!(matches!(follower.tail_once(None), Err(Halt::Rebuild(_))));
    drop(follower);
    let after = readiness(&replica, Some(&expected)).unwrap();
    assert!(matches!(after, Readiness::NotReady { .. }), "{after:?}");
}
