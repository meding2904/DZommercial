//! Differential tests against the Go `internal/orderstatus` oracle.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use azahar_orderstatus::{STATUSES, can_transition, known};
use serde_json::{Value, json};

struct Oracle {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn spawn() -> Self {
        let backend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../backend");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-orderstatus-secrets"])
            .current_dir(&backend)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|error| panic!("start Go oracle in {}: {error}", backend.display()));
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = child.stdout.take().expect("oracle stdout");
        Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        }
    }

    fn batch(&mut self, requests: &[Value]) -> Vec<Value> {
        let mut responses = Vec::with_capacity(requests.len());
        for request in requests {
            {
                let stdin = self.stdin.as_mut().expect("oracle stdin open");
                serde_json::to_writer(&mut *stdin, request).expect("encode oracle request");
                stdin.write_all(b"\n").expect("write oracle request");
                stdin.flush().expect("flush oracle request");
            }

            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .expect("read oracle response");
            assert!(read > 0, "Go oracle exited before answering");
            responses.push(serde_json::from_str(&line).expect("decode oracle response"));
        }
        responses
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

#[test]
fn order_status_matches_go() {
    let mut oracle = Oracle::spawn();

    let listed = oracle.batch(&[json!({"op": "statuses"})]);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["statuses"], json!(STATUSES));

    let mut candidates: Vec<&str> = STATUSES.to_vec();
    candidates.extend([
        "", "Pending", "PENDING", "refunded", "new", "shipping", " pending", "pending ",
    ]);

    let known_requests: Vec<Value> = candidates
        .iter()
        .map(|status| json!({"op": "known", "status": status}))
        .collect();
    let known_responses = oracle.batch(&known_requests);
    for (status, response) in candidates.iter().zip(&known_responses) {
        assert_eq!(
            response["ok"],
            json!(known(status)),
            "known({status:?}) differs from Go"
        );
    }

    let mut transition_requests = Vec::with_capacity(candidates.len().pow(2));
    let mut pairs = Vec::with_capacity(candidates.len().pow(2));
    for from in &candidates {
        for to in &candidates {
            transition_requests.push(json!({"op": "can_transition", "from": from, "to": to}));
            pairs.push((from, to));
        }
    }
    let transition_responses = oracle.batch(&transition_requests);
    for ((from, to), response) in pairs.iter().zip(&transition_responses) {
        assert_eq!(
            response["ok"],
            json!(can_transition(from, to)),
            "can_transition({from:?}, {to:?}) differs from Go"
        );
    }

    println!(
        "orderstatus differential: 1 statuses call, {} known calls, {} transition calls",
        known_requests.len(),
        transition_requests.len()
    );
}
