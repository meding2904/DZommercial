//! Shared Go oracle process for the desktop API differential tests.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use azahar_desktop_api::ApiError;
use serde_json::Value;

pub struct Oracle {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    pub fn start() -> Self {
        match Command::new("go").arg("version").output() {
            Ok(output) if output.status.success() => {}
            _ => panic!(
                "the Go toolchain is required for the desktop API differential test; install go and re-run"
            ),
        }
        let desktop = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop");
        let mut child = Command::new("go")
            .args(["run", "-tags", "oracle", "./cmd/oracle-api"])
            .current_dir(desktop)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn go oracle");
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = child.stdout.take().expect("oracle stdout");
        Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        }
    }

    pub fn command(&mut self, command: &Value) -> Value {
        writeln!(self.stdin, "{command}").expect("write oracle command");
        self.stdin.flush().expect("flush oracle command");
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .expect("read oracle response");
        assert!(
            read > 0,
            "the oracle exited before answering; check the go build output"
        );
        serde_json::from_str(&line).expect("decode oracle response")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn epoch(seconds: i64) -> SystemTime {
    if seconds < 0 {
        UNIX_EPOCH
            .checked_sub(Duration::from_secs(seconds.unsigned_abs()))
            .unwrap_or(UNIX_EPOCH)
    } else {
        UNIX_EPOCH + Duration::from_secs(seconds as u64)
    }
}

pub fn unix(time: Option<SystemTime>) -> i64 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Default)]
pub struct Checker {
    oracle: Option<Oracle>,
    pub comparisons: usize,
}

impl Checker {
    pub fn oracle(&mut self) -> &mut Oracle {
        self.oracle.get_or_insert_with(Oracle::start)
    }

    pub fn check(&mut self, label: &str, command: Value, rust: Result<Value, ApiError>) {
        let response = self.oracle().command(&command);
        let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
        match rust {
            Ok(value) => {
                assert!(
                    ok,
                    "{label}: rust succeeded but oracle failed: {response}\ncommand: {command}"
                );
                let expected = response.get("value").cloned().unwrap_or(Value::Null);
                assert_eq!(
                    value, expected,
                    "{label}: value mismatch\ncommand: {command}"
                );
            }
            Err(error) => {
                assert!(
                    !ok,
                    "{label}: oracle succeeded but rust failed: {error:?}\ncommand: {command}"
                );
                let expected = response.get("error").cloned().unwrap_or(Value::Null);
                let actual = serde_json::to_value(&error).expect("encode rust error");
                assert_eq!(
                    actual, expected,
                    "{label}: error mismatch\ncommand: {command}"
                );
            }
        }
        self.comparisons += 1;
    }

    pub fn check_ok(&mut self, label: &str, command: Value, value: Value) {
        self.check(label, command, Ok(value));
    }
}
