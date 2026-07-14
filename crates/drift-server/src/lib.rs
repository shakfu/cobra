//! `drift-server` — the authoritative networked server.
//!
//! A server is a [`drift_sim::Session`] plus a socket: it owns the one canonical
//! simulation, accepts client connections, applies their [`Command`](drift_economy::Command)s
//! at tick boundaries, and broadcasts state. See [`server`] for the loop; the wire
//! contract (framing and messages) lives in the shared [`drift_proto`] crate. The
//! design rationale is in `docs/dev/multiplayer.md`.

pub mod server;

pub use server::{Server, ServerConfig};
