//! Reserve every derived worker port, not just the OS-selected base.
use std::io;
use std::net::TcpListener;

fn range(base: TcpListener, count: usize) -> io::Result<Vec<TcpListener>> {
    let address = base.local_addr()?;
    let mut listeners = vec![base];
    for offset in 1..count {
        let port = u16::try_from(offset)
            .ok()
            .and_then(|offset| address.port().checked_add(offset))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "worker port overflow"))?;
        listeners.push(TcpListener::bind((address.ip(), port))?);
    }
    Ok(listeners)
}

pub(super) fn reserve(count: usize) -> Vec<TcpListener> {
    assert!(count > 0);
    // Production uses base + index. Select the base through the OS, but admit
    // it only while the entire requested range can be held simultaneously.
    // A busy successor rejects the candidate; never start a partial fixture.
    for _ in 0..128 {
        let base = TcpListener::bind("127.0.0.1:0").expect("allocate fixture worker base");
        match range(base, count) {
            Ok(listeners) => return listeners,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::AddrInUse | io::ErrorKind::InvalidInput
                ) => {}
            Err(error) => panic!("reserve fixture worker range: {error}"),
        }
    }
    panic!("no complete fixture worker port range after 128 candidates");
}

#[test]
fn occupied_successor_refuses_the_whole_candidate() {
    let mut listeners = reserve(2);
    let occupied = listeners.pop().unwrap();
    let base = listeners.pop().unwrap();
    let address = base.local_addr().unwrap();
    // Force precisely the hosted failure: base is available to this fixture,
    // while the second worker's predictable port belongs to another listener.
    let error = range(base, 2).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    assert!(occupied.local_addr().is_ok());
    // A refused candidate releases its own base, never the competing listener.
    let released = TcpListener::bind(address).unwrap();
    assert!(released.local_addr().is_ok());
}

#[test]
fn full_pool_ports_are_distinct_and_held_together() {
    let listeners = reserve(2);
    let base = listeners[0].local_addr().unwrap();
    for (index, listener) in listeners.iter().enumerate() {
        let address = listener.local_addr().unwrap();
        assert_eq!(address.port(), base.port() + index as u16);
        assert_eq!(
            TcpListener::bind(address).unwrap_err().kind(),
            io::ErrorKind::AddrInUse
        );
    }
}
