.PHONY: test run validate inspect fmt lint check build clean

# CLAUDE.md hard requirement: `make test` runs the full suite.
test:
	cargo test --workspace

build:
	cargo build --workspace

check:
	cargo check --workspace --all-targets

# Load + link the bundled mods, report errors.
validate:
	cargo run -p drift-cli -- validate --mods mods/

# Run the headless economy simulation.
# Override defaults, e.g.: make run TICKS=5000 SEED=7
TICKS ?= 2000
SEED ?= 42
run:
	cargo run -p drift-cli -- run --mods mods/ --scenario scenarios/equilibrium.ron --ticks $(TICKS) --seed $(SEED)

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

clean:
	cargo clean
