use super::*;

#[test]
fn real_loopback_http_reads_authenticate_and_refuse_writes() {
    use std::io::{BufRead as _, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let mut p = profile(Store::open_in_memory().unwrap(), config(&c, &key), &c);
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        tick(1000);
        let auth = policy();
        let token = "synthetic-private-ha-credential";
        let doc = json!({"version":1,"credentials":[{"credential_id":"fixture","principal":WHO,
            "audience":"quipu","token_sha256":format!("{:x}",Sha256::digest(token.as_bytes()))}]});
        let registry = crate::crew_credentials::CredentialRegistry::parse(
            &serde_json::to_vec(&doc).unwrap(),
            "quipu",
        )
        .unwrap();
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut header = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.strip_prefix("Authorization: ") {
                    header = Some(v.trim().to_owned());
                }
            }
            let (status, body) = if first.starts_with("POST /episode ") {
                let decision = crate::http_auth::authorize(true, true, None, header.as_deref());
                assert!(matches!(
                    decision,
                    crate::http_auth::AccessDecision::ReadOnly
                ));
                (403, b"read-only".to_vec())
            } else if let Ok(id) = ReadIdentity::authenticate_named(&registry, header.as_deref()) {
                (200, p.query(&auth, &id, QUERY, 0, 2).unwrap().body)
            } else {
                (401, b"unauthenticated".to_vec())
            };
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    for (path, header, want) in [
        ("/ha/query", "", 401),
        (
            "/ha/query",
            "Authorization: Bearer synthetic-private-ha-credential\r\n",
            200,
        ),
        (
            "/episode",
            "Authorization: Bearer synthetic-private-ha-credential\r\n",
            403,
        ),
    ] {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        write!(stream,"POST {path} HTTP/1.1\r\nHost: localhost\r\n{header}Content-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        let mut body = String::new();
        stream.read_to_string(&mut body).unwrap();
        assert!(body.starts_with(&format!("HTTP/1.1 {want} ")));
        if want == 200 {
            assert!(body.contains("urn:ha:item"));
        }
    }
    handle.join().unwrap();
}
