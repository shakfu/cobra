//! Interactive single-player loop: fly a trader through the living galaxy.
//!
//! This is a thin driver over the simulation's command pipeline — every player
//! action becomes a `Command` applied at a tick boundary, exactly as a networked
//! client would issue it. The loop is generic over its input/output streams so it
//! can be driven by a terminal or by a scripted test.

use std::io::{BufRead, Write};

use drift_core::{CommodityId, Quantity, SystemId};
use drift_economy::{Command, PlayerId, Trader, TraderId, TraderLocation, World};
use drift_mods::Registry;

/// A parsed player action (not yet bound to a specific trader).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Buy(CommodityId, Quantity),
    Sell(CommodityId, Quantity),
    Jump(SystemId),
    Wait(u64),
    Status,
    Map,
    Help,
    Quit,
}

/// Resolve a commodity from a player token: exact id, `core:`-prefixed id, or a
/// case-insensitive display-name match.
fn resolve_commodity(reg: &Registry, token: &str) -> Option<CommodityId> {
    reg.commodities()
        .find(|(cid, def)| {
            let id = reg.commodity_name(*cid);
            id == token
                || id.strip_prefix("core:") == Some(token)
                || def.name.eq_ignore_ascii_case(token)
        })
        .map(|(cid, _)| cid)
}

/// Resolve a system from a player token, the same way.
fn resolve_system(reg: &Registry, token: &str) -> Option<SystemId> {
    reg.systems()
        .find(|s| {
            let id = reg.system_name(s.id);
            id == token
                || id.strip_prefix("core:") == Some(token)
                || s.name.eq_ignore_ascii_case(token)
        })
        .map(|s| s.id)
}

/// Parse a positive quantity token.
fn parse_qty(t: &str) -> Result<u32, String> {
    t.parse::<u32>().map_err(|_| format!("`{t}` is not a number"))
}

/// Parse a line of input into an [`Action`], or a human-readable error.
pub fn parse_action(line: &str, reg: &Registry) -> Result<Action, String> {
    let mut it = line.split_whitespace();
    let Some(verb) = it.next() else {
        return Err("type a command (try `help`)".into());
    };
    let rest: Vec<&str> = it.collect();

    match verb.to_ascii_lowercase().as_str() {
        "buy" | "b" => match rest.as_slice() {
            [c, q] => {
                let cid = resolve_commodity(reg, c).ok_or(format!("no such commodity `{c}`"))?;
                Ok(Action::Buy(cid, parse_qty(q)?))
            }
            _ => Err("usage: buy <commodity> <qty>".into()),
        },
        "sell" => match rest.as_slice() {
            [c, q] => {
                let cid = resolve_commodity(reg, c).ok_or(format!("no such commodity `{c}`"))?;
                Ok(Action::Sell(cid, parse_qty(q)?))
            }
            _ => Err("usage: sell <commodity> <qty>".into()),
        },
        "jump" | "j" => match rest.as_slice() {
            [dest] => {
                let sid = resolve_system(reg, dest).ok_or(format!("no such system `{dest}`"))?;
                Ok(Action::Jump(sid))
            }
            _ => Err("usage: jump <system>".into()),
        },
        "wait" | "w" => {
            let n = match rest.first() {
                Some(t) => parse_qty(t)? as u64,
                None => 1,
            };
            Ok(Action::Wait(n))
        }
        "status" | "s" | "look" | "l" => Ok(Action::Status),
        "map" | "m" => Ok(Action::Map),
        "help" | "h" | "?" => Ok(Action::Help),
        "quit" | "q" | "exit" => Ok(Action::Quit),
        other => Err(format!("unknown command `{other}` (try `help`)")),
    }
}

pub const HELP: &str = "\
Commands:
  buy  <commodity> <qty>   purchase goods at the local market
  sell <commodity> <qty>   sell goods from your hold
  jump <system>            travel to a connected system (risky if laden!)
  wait [n]                 let n ticks pass (default 1)
  status                   show your situation
  map                      list systems and danger
  help                     this text
  quit                     leave the game";

fn find(world: &World, id: TraderId) -> Option<&Trader> {
    world.traders().iter().find(|t| t.id == id)
}

/// Mass of goods currently in a trader's hold.
fn hold_used(reg: &Registry, trader: &Trader) -> u32 {
    trader
        .cargo
        .iter()
        .map(|(c, q)| q * reg.commodity(*c).unit_mass)
        .sum()
}

/// Render the docked player's situation.
fn dashboard(reg: &Registry, world: &World, id: TraderId) -> String {
    let Some(t) = find(world, id) else {
        return "You have no ship.".into();
    };
    let TraderLocation::Docked(sys) = t.location else {
        return "In transit...".into();
    };
    let mut s = String::new();
    let sysdef = reg.system(sys);
    s += &format!("\n-- Tick {} --\n", world.tick_count().get());
    s += &format!(
        "At {} (danger {:.2})    Capital: {} cr\n",
        sysdef.name, sysdef.danger, t.capital
    );
    let cap = reg.ship(t.ship).cargo_capacity;
    if t.cargo.is_empty() {
        s += &format!("Hold: empty [0/{cap}]\n");
    } else {
        let items: Vec<String> = t
            .cargo
            .iter()
            .map(|(c, q)| format!("{} x{}", reg.commodity(*c).name, q))
            .collect();
        s += &format!("Hold: {} [{}/{}]\n", items.join(", "), hold_used(reg, t), cap);
    }
    s += "Market:\n";
    let market = &world.markets()[sys.index()];
    for (cid, good) in market.goods.iter() {
        s += &format!(
            "  {:<12} price {:>6}   stock {:>5}\n",
            reg.commodity(*cid).name,
            good.price,
            good.stock
        );
    }
    let jumps: Vec<String> = sysdef
        .connections
        .iter()
        .map(|c| format!("{}(d{:.2})", reg.system(*c).name, reg.system(*c).danger))
        .collect();
    s += &format!("Jumps: {}\n", jumps.join("  "));
    s
}

fn map_text(reg: &Registry) -> String {
    let mut s = String::from("\nSystems:\n");
    for sys in reg.systems() {
        let conns: Vec<&str> = sys.connections.iter().map(|c| reg.system(*c).name.as_str()).collect();
        s += &format!(
            "  {:<10} danger {:.2}  -> {}\n",
            sys.name,
            sys.danger,
            conns.join(", ")
        );
    }
    s
}

/// After a jump departs, advance ticks until the player is docked again,
/// narrating anything that happened in transit (pirate fights, destruction).
fn resolve_transit(reg: &Registry, world: &mut World, id: TraderId) -> Vec<String> {
    let mut log = Vec::new();
    let mut destroyed = false;
    for _ in 0..100_000 {
        let loc = find(world, id).map(|t| t.location.clone());
        match loc {
            Some(TraderLocation::Docked(sys)) => {
                let name = &reg.system(sys).name;
                log.push(if destroyed {
                    format!("Respawned at {name}.")
                } else {
                    format!("Arrived at {name}.")
                });
                break;
            }
            Some(TraderLocation::Destroyed { .. }) => {
                if !destroyed {
                    log.push("You were ambushed and destroyed! Your cargo is lost.".into());
                    destroyed = true;
                }
                world.tick();
            }
            Some(TraderLocation::InTransit { .. }) => {
                let before = find(world, id).map(|t| t.capital).unwrap_or(0);
                world.tick();
                let after = find(world, id).map(|t| t.capital).unwrap_or(0);
                if after > before {
                    log.push(format!(
                        "Fought off pirates and claimed {} cr in bounties!",
                        after - before
                    ));
                }
            }
            None => break,
        }
    }
    log
}

/// Report the outcome of a just-applied command (from the world's error channel).
fn report_command(world: &World, out: &mut impl Write, success: &str) -> std::io::Result<()> {
    match world.last_command_errors().first() {
        Some(e) => writeln!(out, "  rejected: {e}"),
        None => writeln!(out, "  {success}"),
    }
}

/// Run the interactive loop until EOF or `quit`. Generic over the streams so it is
/// driven by a terminal in the binary and by a `Cursor` in tests.
pub fn run_repl<R: BufRead, W: Write>(
    reg: &Registry,
    world: &mut World,
    player: PlayerId,
    id: TraderId,
    mut input: R,
    mut out: W,
) -> std::io::Result<()> {
    writeln!(out, "{HELP}")?;
    loop {
        // The player is always docked at the prompt.
        if find(world, id).is_none() {
            writeln!(out, "Your ship is gone. Game over.")?;
            break;
        }
        write!(out, "{}", dashboard(reg, world, id))?;
        write!(out, "> ")?;
        out.flush()?;

        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            break; // EOF
        }
        if line.trim().is_empty() {
            continue;
        }

        match parse_action(&line, reg) {
            Err(msg) => writeln!(out, "  {msg}")?,
            Ok(Action::Quit) => break,
            Ok(Action::Help) => writeln!(out, "{HELP}")?,
            Ok(Action::Status) => {} // dashboard reprints next loop
            Ok(Action::Map) => write!(out, "{}", map_text(reg))?,
            Ok(Action::Wait(n)) => {
                world.run(n.max(1));
                writeln!(out, "  {n} tick(s) passed.")?;
            }
            Ok(Action::Buy(c, q)) => {
                world.queue_command(Command::Buy { player, trader: id, commodity: c, qty: q });
                world.tick();
                report_command(world, &mut out, &format!("Bought {} {}.", q, reg.commodity(c).name))?;
            }
            Ok(Action::Sell(c, q)) => {
                world.queue_command(Command::Sell { player, trader: id, commodity: c, qty: q });
                world.tick();
                report_command(world, &mut out, &format!("Sold {} {}.", q, reg.commodity(c).name))?;
            }
            Ok(Action::Jump(dest)) => {
                world.queue_command(Command::Jump { player, trader: id, dest });
                world.tick();
                if let Some(e) = world.last_command_errors().first() {
                    writeln!(out, "  rejected: {e}")?;
                } else {
                    writeln!(out, "  Departing for {}...", reg.system(dest).name)?;
                    for msg in resolve_transit(reg, world, id) {
                        writeln!(out, "  {msg}")?;
                    }
                }
            }
        }
    }
    writeln!(out, "Fair skies, commander.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::Arc;

    use drift_data::{ScenarioDef, TraderSpawn};
    use drift_economy::builtin_pricing;
    use drift_mods::load_and_link;

    use super::*;

    fn reg() -> Arc<Registry> {
        let pricing: HashSet<String> = builtin_pricing().names().map(String::from).collect();
        let mods = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods");
        Arc::new(load_and_link(&mods, &pricing).expect("core mods link"))
    }

    /// A sandbox: no NPC traders, no piracy — deterministic and quiet.
    fn sandbox() -> ScenarioDef {
        ScenarioDef {
            name: "sandbox".into(),
            seed: 1,
            ticks: 0,
            traders: TraderSpawn { count: 0, ship: "core:cobra_mk3".into(), starting_capital: 0 },
            piracy: None,
            risk_aversion: 0.0,
            escort: None,
            navy: None,
        }
    }

    #[test]
    fn parses_actions_and_resolves_names() {
        let r = reg();
        let food = r.commodity_id("core:food").unwrap();
        let leesti = r.system_id("core:leesti").unwrap();
        assert_eq!(parse_action("buy food 10", &r), Ok(Action::Buy(food, 10)));
        assert_eq!(parse_action("b Food 3", &r), Ok(Action::Buy(food, 3)));
        assert_eq!(parse_action("sell core:food 5", &r), Ok(Action::Sell(food, 5)));
        assert_eq!(parse_action("jump leesti", &r), Ok(Action::Jump(leesti)));
        assert_eq!(parse_action("wait 4", &r), Ok(Action::Wait(4)));
        assert_eq!(parse_action("wait", &r), Ok(Action::Wait(1)));
        assert_eq!(parse_action("s", &r), Ok(Action::Status));
        assert_eq!(parse_action("map", &r), Ok(Action::Map));
        assert_eq!(parse_action("quit", &r), Ok(Action::Quit));
        // Errors:
        assert!(parse_action("buy nope 1", &r).is_err());
        assert!(parse_action("buy food xx", &r).is_err());
        assert!(parse_action("buy food", &r).is_err());
        assert!(parse_action("frobnicate", &r).is_err());
    }

    fn spawn_player(r: &Registry, world: &mut World, capital: i64) -> (PlayerId, TraderId) {
        let player = PlayerId(0);
        let ship = r.ship_id("core:cobra_mk3").unwrap();
        let lave = r.system_id("core:lave").unwrap();
        world.queue_command(Command::Spawn { player, ship, at: lave, capital });
        world.tick();
        let id = world.traders().iter().find(|t| t.is_player()).unwrap().id;
        (player, id)
    }

    #[test]
    fn scripted_session_buys_and_sells() {
        let r = reg();
        let pricing = builtin_pricing();
        let mut world = World::new(r.clone(), &sandbox(), 1, &pricing).unwrap();
        let (player, id) = spawn_player(&r, &mut world, 5000);

        let input = Cursor::new(b"buy food 10\nsell food 4\nquit\n".to_vec());
        let mut out: Vec<u8> = Vec::new();
        run_repl(&r, &mut world, player, id, input, &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Bought 10 Food"), "output:\n{text}");
        assert!(text.contains("Sold 4 Food"));
        assert!(text.contains("Fair skies"));
        // Net cargo: 10 bought - 4 sold = 6.
        let food = r.commodity_id("core:food").unwrap();
        let t = world.traders().iter().find(|t| t.id == id).unwrap();
        assert_eq!(t.cargo.get(&food).copied(), Some(6));
    }

    #[test]
    fn scripted_jump_moves_the_player() {
        let r = reg();
        let pricing = builtin_pricing();
        let mut world = World::new(r.clone(), &sandbox(), 1, &pricing).unwrap();
        let (player, id) = spawn_player(&r, &mut world, 5000);

        let input = Cursor::new(b"jump leesti\nquit\n".to_vec());
        let mut out: Vec<u8> = Vec::new();
        run_repl(&r, &mut world, player, id, input, &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Departing for Leesti"), "output:\n{text}");
        assert!(text.contains("Arrived at Leesti"));
        let leesti = r.system_id("core:leesti").unwrap();
        let t = world.traders().iter().find(|t| t.id == id).unwrap();
        assert_eq!(t.location, TraderLocation::Docked(leesti));
    }

    #[test]
    fn rejected_command_is_reported_to_the_player() {
        let r = reg();
        let pricing = builtin_pricing();
        let mut world = World::new(r.clone(), &sandbox(), 1, &pricing).unwrap();
        let (player, id) = spawn_player(&r, &mut world, 100);

        // Cannot afford 100000 food, and cannot jump to an unconnected system.
        let input = Cursor::new(b"buy food 100000\njump tionisla\nquit\n".to_vec());
        let mut out: Vec<u8> = Vec::new();
        run_repl(&r, &mut world, player, id, input, &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("rejected:").count(), 2, "both should be rejected:\n{text}");
    }
}
