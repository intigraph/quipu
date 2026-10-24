//! PRIVATE timing/counters only; never publish this experiment.
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap, time::Instant};
#[derive(Default, Serialize)]
struct Metric {
    calls: u64,
    ns: u128,
}
#[derive(Default, Serialize)]
struct Stats {
    nodes: usize,
    node: Option<usize>,
    timers: BTreeMap<String, Metric>,
    counters: BTreeMap<String, u64>,
}
thread_local! { static ACTIVE: RefCell<Option<Stats>> = const { RefCell::new(None) }; }
pub(crate) struct Root(Instant);
pub(crate) fn start(nodes: usize) -> Root {
    ACTIVE.with(|v| {
        assert!(v.borrow().is_none());
        *v.borrow_mut() = Some(Stats {
            nodes,
            ..Stats::default()
        });
    });
    Root(Instant::now())
}
impl Drop for Root {
    fn drop(&mut self) {
        let elapsed = self.0.elapsed().as_nanos();
        ACTIVE.with(|v| {
            if let Some(mut s) = v.borrow_mut().take() {
                s.timers.insert(
                    "resolution_total".into(),
                    Metric {
                        calls: 1,
                        ns: elapsed,
                    },
                );
                eprintln!("PRIVATE7EK {}", serde_json::to_string(&s).unwrap());
            }
        });
    }
}
pub(crate) struct Timer {
    phase: &'static str,
    start: Option<Instant>,
    node: Option<usize>,
}
pub(crate) fn timer(phase: &'static str) -> Timer {
    ACTIVE.with(|v| {
        let s = v.borrow();
        Timer {
            phase,
            start: s.as_ref().map(|_| Instant::now()),
            node: s.as_ref().and_then(|x| x.node),
        }
    })
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let elapsed = start.elapsed().as_nanos();
            ACTIVE.with(|v| {
                if let Some(s) = v.borrow_mut().as_mut() {
                    let key = match self.node {
                        Some(n) => format!("{}:node{}", self.phase, n),
                        None => self.phase.to_owned(),
                    };
                    let m = s.timers.entry(key).or_default();
                    m.calls += 1;
                    m.ns += elapsed;
                }
            });
        }
    }
}
pub(crate) fn add(key: &'static str, n: usize) {
    ACTIVE.with(|v| {
        if let Some(s) = v.borrow_mut().as_mut() {
            let key = match s.node {
                Some(n) => format!("{}:node{}", key, n),
                None => key.to_owned(),
            };
            *s.counters.entry(key).or_default() += n as u64;
        }
    });
}
pub(crate) struct Node(Option<usize>);
pub(crate) fn node(n: usize) -> Node {
    ACTIVE.with(|v| {
        let mut s = v.borrow_mut();
        let s = s.as_mut().unwrap();
        let old = s.node.replace(n);
        Node(old)
    })
}
impl Drop for Node {
    fn drop(&mut self) {
        ACTIVE.with(|v| {
            if let Some(s) = v.borrow_mut().as_mut() {
                s.node = self.0;
            }
        });
    }
}
