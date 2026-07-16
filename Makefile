.PHONY: test run play client observe server validate inspect fmt lint check build clean

# CLAUDE.md hard requirement: `make test` runs the full suite.
test:
	cargo test --workspace

build:
	cargo build --workspace

check:
	cargo check --workspace --all-targets

# Shared content/scenario inputs. Override on any target, e.g.:
#   make play SCENARIO=scenarios/frontier.ron
MODS ?= mods/
SCENARIO ?= scenarios/equilibrium.ron
SEED ?= 42
ADDR ?= 127.0.0.1:4000

# Load + link the bundled mods, report errors.
validate:
	cargo run -p drift-cli -- validate --mods $(MODS)

# Run the headless economy simulation.
# Override defaults, e.g.: make run TICKS=5000 SEED=7
TICKS ?= 2000
run:
	cargo run -p drift-cli -- run --mods $(MODS) --scenario $(SCENARIO) --ticks $(TICKS) --seed $(SEED)

# Play the real-time flight game (Bevy). Fly your trader around its star system:
# W/S thrust, arrows pitch/yaw, Q/E roll, 1-9 jump. Requires the `gui` feature.
play:
	cargo run -p drift-flight --features gui -- $(SCENARIO) $(MODS)

# Graphical observer of the living galaxy (egui/eframe), simulating in-process.
client:
	cargo run -p drift-client -- --mods $(MODS) --scenario $(SCENARIO) --seed $(SEED)

# Graphical observer attached to a running server instead of simulating locally.
# Point it at a host with: make observe ADDR=127.0.0.1:4000
observe:
	cargo run -p drift-client -- --mods $(MODS) --connect $(ADDR)

# Host an authoritative galaxy for networked clients to observe.
server:
	cargo run -p drift-server -- --mods $(MODS) --scenario $(SCENARIO) --seed $(SEED) --addr $(ADDR)

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

clean:
	cargo clean
