//! Shared by the suspend group's unit tests: a fake Reborn control socket in the fixture root
//! (what Python's tests got by patching `suspend_record.socket.socket` or `application_rpc`).
// SPDX-License-Identifier: GPL-2.0-only
use crate::testing::Fixture;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};

/// Answers one request: (op, dark) to the whole answer line, or `None` to close without one.
pub type Handler = Box<dyn FnMut(&str, bool) -> Option<Value> + Send>;

/// An acknowledgement carrying `result`.
pub fn answer(result: Value) -> Option<Value> {
    Some(json!({"version": 1, "id": 6582, "ok": true, "result": result}))
}

/// Reborn's refusal (Python's tests: `application_rpc` raising `ValueError(error)`).
pub fn refusal(error: &str) -> Option<Value> {
    Some(json!({"version": 1, "id": 6582, "ok": false, "error": error}))
}

/// `/run/reborn/control.sock` below a fixture root: every request's op (`:dark` added for a dark
/// restore) is kept in `ops`, in order, before it is answered.
pub struct Reborn {
    pub ops: Arc<Mutex<Vec<String>>>,
    handler: Arc<Mutex<Handler>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl Reborn {
    pub fn serve(fixture: &Fixture, handler: Handler) -> Reborn {
        let path = fixture.path("/run/reborn/control.sock");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let ops = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(Mutex::new(handler));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (ops, handler, stop) = (ops.clone(), handler.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let Ok((stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut line = String::new();
                    if BufReader::new(&stream).read_line(&mut line).is_err() {
                        continue;
                    }
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let op = request["command"]["op"].as_str().unwrap().to_owned();
                    let dark = request["command"]["dark"] == json!(true);
                    ops.lock()
                        .unwrap()
                        .push(format!("{op}{}", if dark { ":dark" } else { "" }));
                    let reply = (handler.lock().unwrap())(&op, dark);
                    if let Some(reply) = reply {
                        let _ = (&stream).write_all(format!("{reply}\n").as_bytes());
                    }
                }
            })
        };
        Reborn {
            ops,
            handler,
            stop,
            thread: Some(thread),
            path,
        }
    }

    /// Reborn as Python's tests answer by default: ready, the screen off for a preparation or a
    /// dark restore, lit after any other restore.
    pub fn ready(fixture: &Fixture) -> Reborn {
        Reborn::serve(fixture, Box::new(ready))
    }

    /// Answer from now on with `handler`.
    pub fn handle(&self, handler: Handler) {
        *self.handler.lock().unwrap() = handler;
    }

    pub fn ops(&self) -> Vec<String> {
        self.ops.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.ops.lock().unwrap().clear();
    }
}

/// The default acknowledgement (`application` in test_product_auto_sleep.py).
pub fn ready(op: &str, dark: bool) -> Option<Value> {
    answer(json!({"sleep_ready": true, "screen_off": op == "sleep_prepare" || dark}))
}

impl Drop for Reborn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
