//! What `ai_probe_local` puts on the wire.
//!
//! ADR-040 section 4 and ADR-031 rule 2.7 rest on the claim that the local
//! runtime probe carries no credential, no note text and no header beyond what
//! the HTTP client adds to every request. The unit tests assert the client is
//! built without a key; this one binds the two ports the probe knocks on,
//! records the raw bytes, and reads them back.
//!
//! The ports are the real ones (11434 and 1234), so the test skips itself when
//! something is already listening there.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use writ_tauri_lib::commands::ai::ai_probe_local;

/// Reads one request off a connection and answers 200, returning the bytes.
fn record_one(listener: TcpListener) -> String {
    let Ok((mut stream, _)) = listener.accept() else {
        return String::new();
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let read = stream.read(&mut buf).unwrap_or(0);
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
    let _ = stream.flush();
    String::from_utf8_lossy(&buf[..read]).to_string()
}

fn port_is_free(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}")
            .parse()
            .expect("a loopback addr"),
        Duration::from_millis(200),
    )
    .is_err()
}

#[test]
fn the_local_probe_sends_no_credential_and_no_header_of_its_own() {
    if !port_is_free(11434) || !port_is_free(1234) {
        eprintln!("skipped: a local runtime is already listening on 11434 or 1234");
        return;
    }
    let ollama = match TcpListener::bind("127.0.0.1:11434") {
        Ok(l) => l,
        Err(_) => {
            eprintln!("skipped: port 11434 could not be bound");
            return;
        }
    };
    let lmstudio = match TcpListener::bind("127.0.0.1:1234") {
        Ok(l) => l,
        Err(_) => {
            eprintln!("skipped: port 1234 could not be bound");
            return;
        }
    };

    let (tx, rx) = mpsc::channel();
    for (name, listener) in [("ollama", ollama), ("lmstudio", lmstudio)] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send((name, record_one(listener)));
        });
    }
    drop(tx);

    let probe = tauri::async_runtime::block_on(ai_probe_local());
    assert!(
        probe.ollama,
        "the probe should see the ollama socket answer"
    );
    assert!(
        probe.lmstudio,
        "the probe should see the lmstudio socket answer"
    );

    let mut seen = Vec::new();
    while let Ok((name, raw)) = rx.recv_timeout(Duration::from_secs(5)) {
        assert!(!raw.is_empty(), "{name}: nothing was recorded");
        let lower = raw.to_ascii_lowercase();

        // The request line names the path ADR-040 section 4 records.
        let request_line = raw.lines().next().unwrap_or_default().to_string();
        let expected_path = if name == "ollama" {
            "/api/tags"
        } else {
            "/v1/models"
        };
        assert!(
            request_line.starts_with(&format!("GET {expected_path} ")),
            "{name}: unexpected request line {request_line:?}"
        );

        // No credential of any shape.
        for forbidden in ["authorization", "x-api-key", "anthropic-version", "cookie"] {
            assert!(
                !lower.contains(forbidden),
                "{name}: the probe carried a {forbidden} header:\n{raw}"
            );
        }
        assert!(
            !lower.contains("key="),
            "{name}: the probe carried a key in the query:\n{raw}"
        );

        // Whatever headers the HTTP client insists on are recorded here so a
        // change in that set is visible, rather than asserted away.
        let headers: Vec<(String, String)> = raw
            .lines()
            .skip(1)
            .take_while(|line| !line.trim().is_empty())
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
            .filter(|(name, _)| !name.is_empty())
            .collect();
        eprintln!("{name}: headers the client added: {headers:?}");
        eprintln!("{name}: raw request:\n{raw}");
        for (header, value) in &headers {
            assert!(
                matches!(header.as_str(), "host" | "accept" | "user-agent"),
                "{name}: unexpected header {header:?} on a probe:\n{raw}"
            );
            // ADR-040 section 4 names an empty user agent, so the probe is not
            // a client a server can tell apart.
            if header == "user-agent" {
                assert!(
                    value.is_empty(),
                    "{name}: the probe named itself in a user agent: {value:?}"
                );
            }
        }
        seen.push(name);
    }
    seen.sort_unstable();
    assert_eq!(seen, vec!["lmstudio", "ollama"], "both sockets answered");
}
