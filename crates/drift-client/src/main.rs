//! `drift-client` — a graphical observer of the living galaxy (egui/eframe).
//!
//! This is a leaf crate: it depends on the simulation crates and never the
//! reverse, so the sim stays renderer-agnostic. It builds a `World` from a
//! scenario and renders it; see [`app::DriftApp`] for the fixed-timestep loop and
//! the galaxy-map view.

mod app;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use drift_data::ScenarioDef;
use drift_economy::{builtin_pricing, World};
use drift_mods::load_and_link;

#[derive(Parser)]
#[command(name = "drift-client", about = "Graphical observer for the Drift galaxy")]
struct Args {
    #[arg(long, default_value = "mods/")]
    mods: PathBuf,
    #[arg(long, default_value = "scenarios/equilibrium.ron")]
    scenario: PathBuf,
    /// Override the scenario's seed.
    #[arg(long)]
    seed: Option<u64>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let names: HashSet<String> = builtin_pricing().names().map(String::from).collect();
    let reg = Arc::new(
        load_and_link(&args.mods, &names)
            .with_context(|| format!("loading mods from {}", args.mods.display()))?,
    );

    let text = std::fs::read_to_string(&args.scenario)
        .with_context(|| format!("reading scenario {}", args.scenario.display()))?;
    let scn: ScenarioDef = ron::from_str(&text).context("parsing scenario")?;
    let seed = args.seed.unwrap_or(scn.seed);

    let world =
        World::new(reg.clone(), &scn, seed, &builtin_pricing()).context("building world")?;

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Drift",
        native_options,
        Box::new(|_cc| Ok(Box::new(app::DriftApp::new(reg, world)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}
