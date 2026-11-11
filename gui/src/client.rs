//! Connection to hwlensd (docs/protocol.md). A plain thread does the blocking socket I/O,
//! keeps the snapshot, applies deltas and hands typed models to the UI through a channel.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};

use crate::{model::Model, patch::patch};

pub const DEFAULT_SOCKET: &str = "/run/hwlensd/hwlensd.sock";

pub enum Event {
    Model(Arc<Model>),
    Disconnected(String),
    /// Answer to a request, or an error the daemon reported for one.
    Reply { ok: bool, text: String },
}

/// Sends requests on the current connection. Cheap to clone.
#[derive(Clone)]
pub struct Requests {
    writer: Arc<Mutex<Option<UnixStream>>>,
}

impl Requests {
    pub fn send(&self, req: &Value) -> Result<()> {
        let mut guard = self.writer.lock().unwrap();
        let stream = guard.as_mut().context("not connected to hwlensd")?;
        let mut line = serde_json::to_vec(req)?;
        line.push(b'\n');
        stream.write_all(&line).context("send request")?;
        Ok(())
    }

    pub fn signal(&self, pid: u64, signal: i32) -> Result<()> {
        self.send(&json!({"signal": {"pid": pid, "signal": signal}}))
    }
}

pub fn start(path: PathBuf, tx: UnboundedSender<Event>) -> Requests {
    let writer = Arc::new(Mutex::new(None));
    let requests = Requests { writer: writer.clone() };
    thread::Builder::new()
        .name("hwlensd-client".into())
        .spawn(move || {
            loop {
                let why = match session(&path, &writer, &tx) {
                    Ok(()) => "hwlensd closed the connection".to_owned(),
                    Err(e) => format!("{e:#}"),
                };
                *writer.lock().unwrap() = None;
                if tx.unbounded_send(Event::Disconnected(why)).is_err() {
                    return; // the UI is gone
                }
                thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("spawn client thread");
    requests
}

fn session(path: &Path, writer: &Mutex<Option<UnixStream>>, tx: &UnboundedSender<Event>) -> Result<()> {
    let stream = UnixStream::connect(path).with_context(|| {
        format!(
            "hwlensd is not running ({}). Start it with: sudo systemctl enable --now hwlensd.socket",
            path.display()
        )
    })?;
    let mut w = stream.try_clone()?;
    w.write_all(b"{\"hello\":{\"v\":1}}\n")?;
    *writer.lock().unwrap() = Some(w);

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut state = Value::Null;
    let mut seq: Option<u64> = None;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut msg: Value = serde_json::from_str(&line).context("bad message from hwlensd")?;

        if let Some(err) = msg.get("error") {
            let text = err.as_str().unwrap_or("error").to_owned();
            if seq.is_none() {
                bail!("hwlensd: {text}");
            }
            send(tx, Event::Reply { ok: false, text })?;
            continue;
        }
        if let Some(what) = msg.get("reply") {
            let ok = msg["ok"].as_bool().unwrap_or(false);
            let err = msg["error"].as_str().unwrap_or_default();
            let what = what.as_str().unwrap_or("request");
            let text = if ok { format!("{what}: ok") } else { format!("{what}: {err}") };
            send(tx, Event::Reply { ok, text })?;
            continue;
        }

        let s = msg["seq"].as_u64().context("message without seq")?;
        let update_ms = msg["update_ms"].as_u64().unwrap_or(1000);
        if let Some(full) = msg.get_mut("full") {
            state = full.take();
        } else {
            let prev = seq.context("delta before the full snapshot")?;
            if s != prev + 1 {
                bail!("lost a message (seq {prev} -> {s}), reconnecting");
            }
            if let Some(delta) = msg.get("delta") {
                patch(&mut state, delta)?;
            } else {
                seq = Some(s);
                continue; // nothing changed
            }
        }
        seq = Some(s);
        send(tx, Event::Model(Arc::new(Model::from_snapshot(&state, update_ms))))?;
    }
}

fn send(tx: &UnboundedSender<Event>, ev: Event) -> Result<()> {
    tx.unbounded_send(ev).map_err(|_| anyhow::anyhow!("the UI is gone"))
}
