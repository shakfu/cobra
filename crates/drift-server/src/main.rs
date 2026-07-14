//! `drift-server` binary — host a galaxy for networked clients.
//!
//! Loads content and a scenario into a [`Session`], binds a TCP listener, and
//! runs the authoritative server loop until the process is signalled.

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use drift_server::{Server, ServerConfig};
use drift_sim::Session;

#[derive(Parser, Debug)]
#[command(name = "drift-server", about = "Authoritative server for the drift galaxy")]
struct Args {
    /// Directory of mods to load.
    #[arg(long, default_value = "mods")]
    mods: PathBuf,
    /// Scenario file to run.
    #[arg(long, default_value = "scenarios/equilibrium.ron")]
    scenario: PathBuf,
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1:4000")]
    addr: String,
    /// Override the scenario's RNG seed.
    #[arg(long)]
    seed: Option<u64>,
    /// Simulation ticks per second.
    #[arg(long, default_value_t = 4.0)]
    tick_hz: f64,
    /// Send a full snapshot every N ticks (events go out every tick).
    #[arg(long, default_value_t = 5)]
    snapshot_every: u64,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let session = Session::load(&args.mods, &args.scenario, args.seed)
        .with_context(|| format!("loading scenario {}", args.scenario.display()))?;

    let listener = TcpListener::bind(&args.addr)
        .with_context(|| format!("binding {}", args.addr))?;
    let local = listener.local_addr()?;

    tracing::info!(
        "drift-server listening on {local} at {} Hz (snapshot every {} ticks)",
        args.tick_hz,
        args.snapshot_every
    );
    println!("drift-server listening on {local}");

    let config = ServerConfig {
        tick_hz: args.tick_hz,
        snapshot_every: args.snapshot_every,
    };
    // No graceful in-process shutdown yet; the process is stopped by a signal
    // (Ctrl-C). The flag exists for embedding and tests.
    let shutdown = Arc::new(AtomicBool::new(false));
    Server::new(session, config).run(listener, shutdown)?;
    Ok(())
}
