//! The server loop: a [`Session`] plus a socket.
//!
//! The server is authoritative. It owns the one canonical simulation and is the
//! only thing that advances it. Clients connect over TCP, send [`Command`]s, and
//! receive state; they never touch the world directly. This is the multiplayer
//! model from `docs/dev/multiplayer.md`, and single-player is its N=1 case.
//!
//! Concurrency (std threads only, no async runtime — the simulation is turn-like
//! at a low tick rate, so this is enough and stays trivially testable):
//!
//! - one **accept thread** takes new connections;
//! - one **reader thread per client** decodes incoming [`ClientMessage`]s;
//! - the **sim thread** (this function) owns the `Session`. It selects between
//!   "a client input arrived" and "the next tick is due" via `recv_timeout`,
//!   applies inputs at the tick boundary, and broadcasts state after each tick.
//!
//! All cross-thread traffic flows through one channel of [`Input`] events, so the
//! sim thread mutates the world single-threaded and stays deterministic. Wall-clock
//! is used only to *schedule* ticks; it never enters simulation logic.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use drift_economy::Command;
use drift_proto::{encode, read_msg, write_msg, ClientMessage, ServerMessage};
use drift_sim::Session;

/// Server tuning.
#[derive(Debug, Clone, Copy)]
pub struct ServerConfig {
    /// Simulation ticks per second. The economy is turn-like, so this is low.
    pub tick_hz: f64,
    /// Send a full snapshot every this many ticks (in addition to per-tick
    /// events). `1` sends one every tick; larger values trade freshness for
    /// bandwidth. A snapshot is always sent to a client the moment it connects.
    pub snapshot_every: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            tick_hz: 4.0,
            snapshot_every: 5,
        }
    }
}

/// An event delivered to the sim thread from the network side. Funnelling
/// connects, commands, and disconnects through one channel keeps the world
/// mutated by exactly one thread.
enum Input {
    /// A client connected; carries the write half of its socket and the id the
    /// accept thread assigned it.
    Connect(u64, TcpStream),
    /// A client issued a command (already deserialized).
    Command(Command),
    /// A client's reader thread ended (disconnect or protocol error).
    Disconnect(u64),
}

/// A connected client the sim thread broadcasts to.
struct Client {
    id: u64,
    stream: TcpStream,
}

/// The authoritative server. Owns a [`Session`]; [`run`](Server::run) drives it.
pub struct Server {
    session: Session,
    config: ServerConfig,
}

impl Server {
    pub fn new(session: Session, config: ServerConfig) -> Self {
        Self { session, config }
    }

    /// Run the server on `listener` until `shutdown` is set. Blocks the calling
    /// thread (it becomes the sim thread). The accept and per-client reader
    /// threads are spawned internally. Returns when `shutdown` is observed.
    pub fn run(mut self, listener: TcpListener, shutdown: Arc<AtomicBool>) -> std::io::Result<()> {
        let (tx, rx) = mpsc::channel::<Input>();

        // Accept thread: non-blocking accept + short sleep so it can observe
        // `shutdown` rather than parking forever inside `accept()`.
        listener.set_nonblocking(true)?;
        let accept_shutdown = shutdown.clone();
        let accept_tx = tx.clone();
        let accept = thread::spawn(move || accept_loop(listener, accept_tx, accept_shutdown));

        let period = Duration::from_secs_f64(1.0 / self.config.tick_hz.max(0.001));
        let snapshot_every = self.config.snapshot_every.max(1);
        let mut clients: Vec<Client> = Vec::new();
        let mut next_tick = Instant::now() + period;

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            let now = Instant::now();
            if now >= next_tick {
                // Advance one tick (applies commands queued since the last tick
                // in their arrival order) and broadcast the result.
                let events = self.session.step();
                let tick = self.session.world().tick_count().get();
                let snapshot = if tick.is_multiple_of(snapshot_every) {
                    serde_json::to_value(self.session.snapshot()).ok()
                } else {
                    None
                };
                let msg = ServerMessage::State {
                    tick,
                    events,
                    snapshot,
                };
                broadcast(&mut clients, &msg);
                next_tick += period;
                continue;
            }

            match rx.recv_timeout(next_tick - now) {
                Ok(Input::Connect(id, mut stream)) => {
                    // Send the newcomer the current full state immediately, so it
                    // has a baseline before the next delta-free broadcast.
                    let welcome = ServerMessage::State {
                        tick: self.session.world().tick_count().get(),
                        events: Vec::new(),
                        snapshot: serde_json::to_value(self.session.snapshot()).ok(),
                    };
                    if write_msg(&mut stream, &welcome).is_ok() {
                        clients.push(Client { id, stream });
                    }
                }
                Ok(Input::Command(cmd)) => self.session.queue_command(cmd),
                Ok(Input::Disconnect(id)) => clients.retain(|c| c.id != id),
                Err(RecvTimeoutError::Timeout) => {}
                // All senders gone (accept thread and every reader ended). Nothing
                // more can arrive; stop.
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        // The accept thread polls `shutdown` and exits within its sleep interval.
        let _ = accept.join();
        Ok(())
    }
}

/// Accept connections until `shutdown`. Each connection gets an id, a reader
/// thread for its inbound half, and a `Connect` handing its outbound half to the
/// sim thread.
fn accept_loop(listener: TcpListener, tx: Sender<Input>, shutdown: Arc<AtomicBool>) {
    let mut next_id: u64 = 0;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((stream, _addr)) => {
                let id = next_id;
                next_id += 1;
                // Two handles to the same socket: the sim thread writes, the
                // reader thread reads. `try_clone` failing means we skip this
                // client rather than corrupt the loop.
                // An accepted socket can inherit the listener's non-blocking mode
                // on some platforms; force blocking so `write_all`/`read_exact`
                // park instead of erroring with `WouldBlock`.
                stream.set_nonblocking(false).ok();
                stream.set_nodelay(true).ok();
                let read_half = match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if tx.send(Input::Connect(id, stream)).is_err() {
                    break; // sim thread gone
                }
                let rtx = tx.clone();
                thread::spawn(move || reader_loop(id, read_half, rtx));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                // Transient accept error; back off briefly and retry.
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// Decode inbound messages from one client until the stream closes or a protocol
/// error occurs, forwarding commands to the sim thread. Blocks on `read_msg`; it
/// unblocks when the client disconnects (read returns an error), at which point
/// it reports the disconnect and ends. A blocked reader outlives `shutdown`, but
/// that only holds a socket, and the process exit reclaims it.
fn reader_loop(id: u64, mut stream: TcpStream, tx: Sender<Input>) {
    // Loops until the stream closes (read errors) or the sim thread is gone.
    while let Ok(ClientMessage::Command(cmd)) = read_msg::<_, ClientMessage>(&mut stream) {
        if tx.send(Input::Command(cmd)).is_err() {
            break; // sim thread gone
        }
    }
    let _ = tx.send(Input::Disconnect(id));
}

/// Serialize `msg` once and write it to every client, dropping any client whose
/// write fails (a disconnect the sim thread notices before its reader does).
fn broadcast(clients: &mut Vec<Client>, msg: &ServerMessage) {
    let bytes = match encode(msg) {
        Ok(b) => b,
        Err(_) => return,
    };
    clients.retain_mut(|c| c.stream.write_all(&bytes).and_then(|_| c.stream.flush()).is_ok());
}
