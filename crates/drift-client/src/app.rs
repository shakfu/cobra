//! The eframe application: an observer view of the living galaxy.
//!
//! The client owns the `Arc<Registry>` and the `World` and advances the simulation
//! on a **fixed timestep** decoupled from the render frame rate: each frame it
//! accumulates `dt * speed` sim-ticks and runs whole ticks, so pausing and
//! changing speed never affect determinism (the sim only ever advances by whole
//! ticks). Rendering is a pure read of world state — the simulation crates know
//! nothing about egui.

use std::collections::HashMap;
use std::sync::Arc;

use drift_core::{SystemId, Tick};
use drift_economy::{EventCategory, PatrolLocation, SimEvent, TraderLocation, World};
use drift_mods::Registry;
use eframe::egui;

/// Cap on sim ticks executed per frame, so a stall cannot spiral.
const MAX_TICKS_PER_FRAME: u32 = 400;

pub struct DriftApp {
    reg: Arc<Registry>,
    world: World,
    paused: bool,
    /// Simulation speed in ticks per second.
    speed: f64,
    /// Fractional tick accumulator for the fixed-timestep loop.
    accum: f64,
    /// Galaxy coordinate bounds (min_x, min_y, max_x, max_y).
    bounds: (f64, f64, f64, f64),
    /// Which event categories the log panel shows.
    show_combat: bool,
    show_piracy: bool,
    show_navy: bool,
    show_system: bool,
}

impl DriftApp {
    pub fn new(reg: Arc<Registry>, world: World) -> Self {
        let (mut minx, mut miny, mut maxx, mut maxy) =
            (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for s in reg.systems() {
            let [x, y] = s.position;
            minx = minx.min(x);
            miny = miny.min(y);
            maxx = maxx.max(x);
            maxy = maxy.max(y);
        }
        Self {
            reg,
            world,
            paused: false,
            speed: 10.0,
            accum: 0.0,
            bounds: (minx, miny, maxx, maxy),
            show_combat: true,
            show_piracy: true,
            show_navy: true,
            show_system: true,
        }
    }

    /// Whether an event of `category` passes the current log filters.
    fn shows(&self, category: EventCategory) -> bool {
        match category {
            EventCategory::Combat => self.show_combat,
            EventCategory::Piracy => self.show_piracy,
            EventCategory::Navy => self.show_navy,
            EventCategory::System => self.show_system,
        }
    }

    /// Map a galaxy position into the given screen rectangle (y is flipped, since
    /// galaxy y is up and screen y is down).
    fn to_screen(&self, p: [f64; 2], rect: egui::Rect) -> egui::Pos2 {
        let (minx, miny, maxx, maxy) = self.bounds;
        let inner = rect.shrink(50.0);
        let nx = if maxx > minx { (p[0] - minx) / (maxx - minx) } else { 0.5 };
        let ny = if maxy > miny { (p[1] - miny) / (maxy - miny) } else { 0.5 };
        egui::pos2(
            inner.left() + nx as f32 * inner.width(),
            inner.bottom() - ny as f32 * inner.height(),
        )
    }

    /// A dot position fanned around a system node (so co-docked agents don't
    /// overlap). `fan` counts how many agents have already been placed there.
    fn fan_pos(&self, sys: SystemId, fan: &mut HashMap<u32, u32>, rect: egui::Rect) -> egui::Pos2 {
        let i = fan.entry(sys.0).or_insert(0);
        let angle = *i as f32 * 0.9;
        let r = 16.0 + (*i as f32 / 8.0) * 4.0;
        *i += 1;
        let c = self.to_screen(self.reg.system(sys).position, rect);
        egui::pos2(c.x + r * angle.cos(), c.y + r * angle.sin())
    }

    /// Interpolated position of a ship in transit, at fractional tick `now_f`.
    fn transit_pos(
        &self,
        origin: SystemId,
        dest: SystemId,
        departure: Tick,
        arrival: Tick,
        now_f: f64,
        rect: egui::Rect,
    ) -> egui::Pos2 {
        let (d0, d1) = (departure.get() as f64, arrival.get() as f64);
        let prog = if d1 > d0 { ((now_f - d0) / (d1 - d0)).clamp(0.0, 1.0) } else { 1.0 };
        let p0 = self.reg.system(origin).position;
        let p1 = self.reg.system(dest).position;
        let g = [p0[0] + (p1[0] - p0[0]) * prog, p0[1] + (p1[1] - p0[1]) * prog];
        self.to_screen(g, rect)
    }

    fn advance(&mut self, dt: f64) {
        if self.paused {
            return;
        }
        self.accum += dt * self.speed;
        let mut n = 0;
        while self.accum >= 1.0 && n < MAX_TICKS_PER_FRAME {
            self.world.tick();
            self.accum -= 1.0;
            n += 1;
        }
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("hud").min_width(240.0).show(ctx, |ui| {
            ui.heading("Drift");
            ui.label(format!("Tick {}", self.world.tick_count().get()));
            ui.separator();

            ui.horizontal(|ui| {
                let label = if self.paused { "Resume" } else { "Pause" };
                if ui.button(label).clicked() {
                    self.paused = !self.paused;
                }
                if ui.button("Step").clicked() {
                    self.world.tick();
                }
            });
            ui.add(egui::Slider::new(&mut self.speed, 0.0..=60.0).text("ticks/sec"));
            ui.separator();

            let p = self.world.piracy_stats();
            ui.label(format!("Traders:  {}", self.world.traders().len()));
            ui.label(format!("Pirates:  {}", self.world.pirates().len()));
            ui.label(format!("Navy:     {}", self.world.navy().len()));
            ui.separator();
            ui.label(format!("Ambushes:          {}", p.ambushes));
            ui.label(format!("Traders lost:      {}", p.traders_lost));
            ui.label(format!(
                "Pirates destroyed: {}",
                p.pirates_destroyed + p.pirates_suppressed
            ));
            ui.label(format!("Bounties paid:     {}", p.bounties_paid));
            ui.separator();

            ui.label("Nodes: danger (green safe -> red lawless)");
            ui.colored_label(TRADER, "\u{25CF} traders");
            ui.colored_label(PIRATE, "\u{25CF} pirates");
            ui.colored_label(NAVY, "\u{25CF} navy");
            ui.label("(agents shown at the system they are docked at)");
        });
    }

    /// A scrolling, colour-coded event log at the bottom of the window, with
    /// per-category filter checkboxes.
    fn log_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("log")
            .resizable(true)
            .default_height(150.0)
            .show(ctx, |ui| {
                // Filter controls (mutate self before we read events).
                ui.horizontal(|ui| {
                    ui.strong("Event log");
                    ui.separator();
                    ui.checkbox(&mut self.show_combat, "combat");
                    ui.checkbox(&mut self.show_piracy, "piracy");
                    ui.checkbox(&mut self.show_navy, "navy");
                    ui.checkbox(&mut self.show_system, "system");
                });

                let events: Vec<&SimEvent> = self
                    .world
                    .events()
                    .filter(|e| self.shows(e.category))
                    .collect();
                ui.label(format!("({} shown)", events.len()));

                let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show_rows(ui, row_h, events.len(), |ui, range| {
                        for i in range {
                            let e = events[i];
                            ui.colored_label(
                                category_color(e.category),
                                format!("[{:>5}] {}", e.tick.get(), e.message),
                            );
                        }
                    });
            });
    }

    fn galaxy_map(&self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(8, 10, 18));

            // Jump edges (drawn once per pair).
            for s in self.reg.systems() {
                let a = self.to_screen(s.position, rect);
                for &c in &s.connections {
                    if c.0 > s.id.0 {
                        let b = self.to_screen(self.reg.system(c).position, rect);
                        painter.line_segment([a, b], egui::Stroke::new(1.0_f32, egui::Color32::from_gray(55)));
                    }
                }
            }

            // System nodes.
            for s in self.reg.systems() {
                let c = self.to_screen(s.position, rect);
                painter.circle_filled(c, 10.0, danger_color(s.danger));
                painter.circle_stroke(c, 10.0, egui::Stroke::new(1.0_f32, egui::Color32::WHITE));
                painter.text(
                    egui::pos2(c.x, c.y - 14.0),
                    egui::Align2::CENTER_BOTTOM,
                    &s.name,
                    egui::FontId::proportional(13.0),
                    egui::Color32::from_gray(220),
                );
            }

            // Agents: docked ones fanned around their node, in-transit ones
            // interpolated along their jump edge at the current fractional tick.
            let now_f = self.world.tick_count().get() as f64 + self.accum;
            let mut fan: HashMap<u32, u32> = HashMap::new();

            for t in self.world.traders() {
                let pos = match t.location {
                    TraderLocation::Docked(sys) => Some(self.fan_pos(sys, &mut fan, rect)),
                    TraderLocation::InTransit { origin, dest, departure, arrival } => {
                        Some(self.transit_pos(origin, dest, departure, arrival, now_f, rect))
                    }
                    TraderLocation::Destroyed { .. } => None,
                };
                if let Some(p) = pos {
                    painter.circle_filled(p, 2.5, TRADER);
                }
            }

            for (fleet, color) in [
                (self.world.pirates(), PIRATE),
                (self.world.navy(), NAVY),
            ] {
                for pat in fleet {
                    let p = match pat.location {
                        PatrolLocation::Docked(sys) => self.fan_pos(sys, &mut fan, rect),
                        PatrolLocation::InTransit { origin, dest, departure, arrival } => {
                            self.transit_pos(origin, dest, departure, arrival, now_f, rect)
                        }
                    };
                    painter.circle_filled(p, 2.5, color);
                }
            }
        });
    }
}

const TRADER: egui::Color32 = egui::Color32::from_rgb(90, 160, 255);
const PIRATE: egui::Color32 = egui::Color32::from_rgb(230, 70, 70);
const NAVY: egui::Color32 = egui::Color32::from_rgb(80, 220, 220);

fn category_color(c: EventCategory) -> egui::Color32 {
    match c {
        EventCategory::Combat => egui::Color32::from_rgb(240, 180, 70),
        EventCategory::Piracy => egui::Color32::from_rgb(235, 90, 90),
        EventCategory::Navy => egui::Color32::from_rgb(90, 210, 210),
        EventCategory::System => egui::Color32::from_gray(170),
    }
}

fn danger_color(d: f64) -> egui::Color32 {
    let d = d.clamp(0.0, 1.0) as f32;
    let r = (60.0 + d * (220.0 - 60.0)) as u8;
    let g = (200.0 - d * (200.0 - 40.0)) as u8;
    egui::Color32::from_rgb(r, g, 60)
}

impl eframe::App for DriftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.stable_dt) as f64;
        self.advance(dt);
        self.side_panel(ctx);
        self.log_panel(ctx);
        self.galaxy_map(ctx);
        // Keep animating (drives the fixed-timestep sim each frame).
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use drift_data::{ScenarioDef, TraderSpawn};
    use drift_economy::builtin_pricing;
    use drift_mods::load_and_link;

    use super::*;

    fn app() -> DriftApp {
        let names: HashSet<String> = builtin_pricing().names().map(String::from).collect();
        let mods = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods");
        let reg = Arc::new(load_and_link(&mods, &names).unwrap());
        let scn = ScenarioDef {
            name: "t".into(),
            seed: 1,
            ticks: 0,
            traders: TraderSpawn { count: 0, ship: String::new(), starting_capital: 0 },
            piracy: None,
            risk_aversion: 0.0,
            escort: None,
            navy: None,
        };
        let world = World::new(reg.clone(), &scn, 1, &builtin_pricing()).unwrap();
        DriftApp::new(reg, world)
    }

    #[test]
    fn fixed_timestep_advances_by_dt_times_speed() {
        let mut a = app();
        a.speed = 10.0;
        a.advance(1.0); // 1s * 10 ticks/s = 10 ticks
        assert_eq!(a.world.tick_count().get(), 10);
        // Fractions carry across frames rather than being lost.
        a.advance(0.05); // +0.5 tick -> none yet
        assert_eq!(a.world.tick_count().get(), 10);
        a.advance(0.05); // +0.5 -> one whole tick
        assert_eq!(a.world.tick_count().get(), 11);
    }

    #[test]
    fn paused_does_not_advance() {
        let mut a = app();
        a.paused = true;
        a.advance(10.0);
        assert_eq!(a.world.tick_count().get(), 0);
    }

    #[test]
    fn advance_is_capped_per_frame() {
        let mut a = app();
        a.speed = 1000.0;
        a.advance(100.0); // ~100k ticks requested; capped to avoid a spiral
        assert_eq!(a.world.tick_count().get() as u32, MAX_TICKS_PER_FRAME);
    }
}
