//! The command pipeline — the multiplayer-ready entry point for player actions.
//!
//! Every action a player takes is a serializable [`Command`] queued via
//! [`World::queue_command`](crate::World::queue_command) and applied, validated, at
//! a tick boundary (the `command_phase`, which runs first each tick). Modelling
//! actions this way is the load-bearing provision for multiplayer: single-player
//! enqueues locally, a server would enqueue from the network, and in both cases the
//! world applies commands deterministically at the tick. Commands never mutate the
//! world directly, so they can be ordered, replayed, and rejected.
//!
//! See `docs/dev/multiplayer.md` for the surrounding design.

use drift_core::{CommodityId, Money, Quantity, ShipId, SystemId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::trader::TraderId;

/// Identifies a player. Player 0 is a perfectly good single-player convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u32);

/// Who controls an agent. NPC agents run the built-in AI; player-owned agents act
/// only on commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Owner {
    #[default]
    Npc,
    Player(PlayerId),
}

/// A player-issued action. Operands are resolved handles (the client/server maps
/// names to ids before issuing), and the whole type is serde-serializable, i.e.
/// already wire-ready. Traders are addressed by a stable [`TraderId`]: a client
/// spawns a ship, reads its server-assigned id from the next world state, and uses
/// that id thereafter — robust across other traders being added or removed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Bring a new player-owned trader into the galaxy. Its id is assigned by the
    /// world and observed in the resulting state.
    Spawn {
        player: PlayerId,
        ship: ShipId,
        at: SystemId,
        capital: Money,
    },
    /// Retire a player trader, removing it from the galaxy.
    Despawn { player: PlayerId, trader: TraderId },
    /// Order a docked player trader to jump to a connected system.
    Jump {
        player: PlayerId,
        trader: TraderId,
        dest: SystemId,
    },
    /// Buy `qty` of a commodity at the trader's current market.
    Buy {
        player: PlayerId,
        trader: TraderId,
        commodity: CommodityId,
        qty: Quantity,
    },
    /// Sell `qty` of a commodity from the hold at the trader's current market.
    Sell {
        player: PlayerId,
        trader: TraderId,
        commodity: CommodityId,
        qty: Quantity,
    },
}

impl Command {
    /// The player that issued the command (for authorization).
    pub fn player(&self) -> PlayerId {
        match *self {
            Command::Spawn { player, .. }
            | Command::Despawn { player, .. }
            | Command::Jump { player, .. }
            | Command::Buy { player, .. }
            | Command::Sell { player, .. } => player,
        }
    }
}

/// Why a command was rejected. Rejection is normal (input is untrusted), never
/// fatal: the command is dropped and counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CommandError {
    #[error("unknown ship id")]
    UnknownShip,
    #[error("unknown or invalid system id")]
    InvalidSystem,
    #[error("no such trader")]
    UnknownTrader,
    #[error("trader is not owned by this player")]
    NotOwner,
    #[error("trader is not docked")]
    NotDocked,
    #[error("destination is not reachable in one jump")]
    Unreachable,
    #[error("this market does not trade that commodity")]
    UnknownGood,
    #[error("not enough capital")]
    InsufficientFunds,
    #[error("not enough stock on the market")]
    InsufficientStock,
    #[error("not enough of that good in the hold")]
    InsufficientCargo,
    #[error("would exceed cargo capacity")]
    OverCapacity,
    #[error("quantity must be positive")]
    ZeroQuantity,
}
