//! Solution dumps: a solution, ours or another solver's, in a solver-neutral JSON format, so
//! that two solvers' answers to the same spot can be compared.
//!
//! A dump is the contract between this solver and a wrapper program around a reference solver,
//! which may be written by someone who has never seen this code. So it names everything by what
//! it is, never by an index that another solver might order differently: a node by the actions
//! and cards that lead to it, an action by its kind and amount, and a hand by its two cards. It
//! also carries the spot it answers, so that a dump of another spot, or of another tree, is
//! caught before any number in it is read.
//!
//! [`SolutionDump::from_solution`] makes the dump of one of our solutions, and
//! [`SolutionDump::to_json`] writes it. [`SolutionDump::from_json`] and [`SolutionDump::load`]
//! read a dump and check it on its own; [`MappedSolution::from_dump`] then checks it against a
//! spot and maps it onto our tree. Version 1 holds spots that start on the river, and is
//! described first, in full. Version 2 holds spots that start on the flop or the turn, and is
//! described [after it](#version-2-spots-that-start-on-the-flop-or-the-turn) as the changes it
//! makes to version 1.
//!
//! # Example
//!
//! Abridged from the dump of `toy_polarized`, whose OOP range has 22 hands and IP's 12:
//!
//! ```json
//! {
//!   "format": "pk-solution-dump",
//!   "version": 1,
//!   "solver": {"name": "pk-postflop", "iterations": 310, "exploitability": 0.27},
//!   "spot": {
//!     "name": "toy_polarized",
//!     "board": "Ks9h6d3c2s",
//!     "pot": 1000,
//!     "effective_stack": 2000,
//!     "rake": {"percent": 0.0, "cap": 0},
//!     "hands": {
//!       "oop": {"9s9d": 1.0, "JsTs": 1.0, "JhTs": 1.0},
//!       "ip": {"KcQs": 1.0, "KhQh": 1.0}
//!     }
//!   },
//!   "values": {"oop": 363.64, "ip": 636.36},
//!   "nodes": [
//!     {
//!       "path": [],
//!       "player": "oop",
//!       "actions": [{"kind": "check"}, {"kind": "bet", "amount": 500}],
//!       "strategy": {"9s9d": [0.0, 1.0], "JsTs": [0.875, 0.125], "JhTs": [1.0, 0.0]},
//!       "ev": {"9s9d": 1333.33, "JsTs": 0.0, "JhTs": 0.0}
//!     },
//!     {
//!       "path": [{"kind": "bet", "amount": 500}],
//!       "player": "ip",
//!       "actions": [{"kind": "fold"}, {"kind": "call", "amount": 500}],
//!       "strategy": {"KcQs": [0.33, 0.67], "KhQh": [0.33, 0.67]},
//!       "ev": {"KcQs": 0.0, "KhQh": 0.0}
//!     }
//!   ]
//! }
//! ```
//!
//! The whole file lists every hand of each range and every decision node: the root, IP's node
//! after a check, IP's node facing the bet, and OOP's node facing a bet after checking.
//!
//! # Conventions
//!
//! - **Chips.** Every amount is a whole number of chips, with 1bb = 100 chips: the pot, the
//!   stack, the rake cap and every action's amount. So are the EVs and values, though they need
//!   not be whole.
//! - **Players.** `"oop"` acts first on the river and `"ip"` last.
//! - **Amounts.** An action's amount is the acting player's total for the street once the
//!   action is taken: on the river, all it has put in since the spot started. A call's amount
//!   is the bet or raise it calls; a raise's is the total raised to, not the increment. Fold and
//!   check have none. All-in is not a kind of its own: it is the bet or raise whose amount is
//!   the player's whole `effective_stack`. These are the amounts of the `to` field of
//!   [`pk_game::Action`], and the street totals that `reference/README.md` lists per spot.
//! - **Hands.** A hand is its two cards, written one after the other, such as `AsKs`: a rank
//!   from `23456789TJQKA` then a suit from `cdhs`. The cards may come in either order and in
//!   either case. Each player's hands are the combos of its range with a positive weight that
//!   share no card with the board, each with its range weight from 0 to 1.
//! - **The deal.** OOP is dealt `h` and IP `o` with probability proportional to
//!   `weight(h) · weight(o)` when the two share no card, and never otherwise. "Per hand dealt"
//!   means averaged over this deal.
//! - **Payoffs.** A player's payoff is the chips it ends the hand with less the chips it had
//!   when the spot started: the pot at the start belongs to neither player. Winning a showdown
//!   with `c` put in each pays `pot + c − rake`, losing it `−c`, and a split pays
//!   `(pot − rake) / 2`. After a fold the winner gets `pot + c − rake`, where `c` is what the
//!   folder put in, and the folder `−c`; the uncalled part of the winner's last bet goes back to
//!   it and is not raked. The rake is `min(cap, percent / 100 × raked pot)` on the raked pot
//!   `pot + 2c`, in fractions of a chip ([`crate::payoff`]). So the two players' payoffs sum to
//!   the pot less the rake, not to zero.
//! - **EVs.** At a node where player `i` acts, the EV of its hand `h` is its expected payoff,
//!   counted as above from the start of the spot (not from the node), given that play has
//!   reached the node and that `i` holds `h`, when both players follow the dump's strategy from
//!   there on. The opponent's hand is weighted by its range weight and its own probability of
//!   playing to the node, among the hands that share no card with `h`. `i`'s own probability of
//!   playing to the node does not enter. Where no hand of the opponent that `h` allows plays to
//!   the node, the EV is undefined and is written `null`.
//!   [`crate::node_values`](mod@crate::node_values) gives the formula.
//! - **Values.** A player's value is its expected payoff per hand dealt, the average of its
//!   EVs at the root over the deal. The two sum to the pot less the expected rake.
//! - **Per hand only.** A dump carries each hand's probabilities and EVs, and no frequency or
//!   EV summed over a range, a class or a node. Solvers sum a range in different ways (with or
//!   without card removal by the opponent's range, for one), and the sums can differ by several
//!   points, so every aggregate is computed by the comparison
//!   ([`crate::compare`](mod@crate::compare)), by the same code for both solutions. Do not try
//!   to write a range view's percentages into a dump:
//!   there is no place for them. The only figures over a whole range are `values` and
//!   `solver.exploitability`, which are reported, not relied on: the comparison checks
//!   `values` against what the strategy gives, and measures both itself.
//!
//! A solver whose EVs count from another baseline converts before writing. One that counts
//! from the node, so that the chips a player put in on the way are sunk, adds `−c` for the
//! chips `c` the acting player has put in at the node. One that scores the spot as zero-sum,
//! by crediting each player half the starting pot, adds `pot / 2`. One whose amounts are in big
//! blinds multiplies by 100.
//!
//! # Fields
//!
//! The top level holds six keys:
//!
//! - `format` is the string `"pk-solution-dump"` and `version` the integer 1.
//! - `solver` names the solver that made the strategy: `name`, required and not blank;
//!   `iterations`, a whole number, if it has one; and `exploitability`, its own figure for the
//!   strategy's exploitability in chips per hand dealt, as it defines and measures it. That
//!   figure is reported, never relied on: the comparison measures every strategy itself.
//! - `spot` is the spot the dump answers: `name`, if any; `board`, its five cards; `pot`, the
//!   pot at the start of the river, and `effective_stack`, what each player has left to bet
//!   then, both positive; `rake`, with `percent`, at least 0 and less than 100, and `cap`, in
//!   chips; and `hands`, which maps `oop` and `ip` each to an object from hand to weight.
//! - `values`, optional, holds `oop` and `ip`, each player's value.
//! - `nodes` lists every decision node of the tree once, in any order. Each node holds:
//!   - `path`, the actions from the root to it, oldest first, as action objects: `[]` at the
//!     root;
//!   - `player`, who acts there;
//!   - `actions`, the actions it offers, each `{"kind": k}` for `fold` and `check`, or
//!     `{"kind": k, "amount": n}` for `call`, `bet` and `raise`, in any order;
//!   - `strategy`, which maps each hand of the acting player to its probabilities, one per
//!     action in the order of `actions`. Every hand has a row at every node, even one that
//!     never plays to it;
//!   - `ev`, optional, which maps each hand of the acting player to its EV or `null`. Either
//!     every node has `ev` or none does.
//!
//! An optional key may also be written as `null`, which counts as leaving it out.
//!
//! # Version 2: spots that start on the flop or the turn
//!
//! Version 2 is version 1 with the changes below, for a spot whose board has three or four
//! cards. Everything not mentioned here is as in version 1: the keys, the players, the hand
//! names, the payoff and EV conventions, the rake, what `values` and `solver` hold. A river spot
//! is still written as version 1, unchanged; a flop or turn spot is written as version 2.
//!
//! Abridged from a dump of a turn spot, with its three hands of OOP's and two of IP's: OOP's
//! root, IP's node after a check, and OOP's first river node after both check and the 9s
//! falls, where `9s9h` has no row because it holds the 9s:
//!
//! ```json
//! {
//!   "format": "pk-solution-dump",
//!   "version": 2,
//!   "solver": {"name": "pk-postflop", "iterations": 60, "exploitability": 2.8},
//!   "spot": {
//!     "name": "turn_example",
//!     "board": "Kh9c7d2s",
//!     "pot": 1000,
//!     "effective_stack": 1500,
//!     "rake": {"percent": 5.0, "cap": 100},
//!     "hands": {
//!       "oop": {"9s9h": 1.0, "AhTh": 1.0, "QhJh": 0.5},
//!       "ip": {"KcQc": 1.0, "AhAd": 1.0}
//!     }
//!   },
//!   "complete": true,
//!   "values": {"oop": 512.4, "ip": 437.6},
//!   "nodes": [
//!     {
//!       "path": [],
//!       "player": "oop",
//!       "actions": [{"kind": "check"}, {"kind": "bet", "amount": 500},
//!                   {"kind": "bet", "amount": 1500}],
//!       "strategy": {"9s9h": [0.4, 0.6, 0.0], "AhTh": [1.0, 0.0, 0.0], "QhJh": [0.0, 1.0, 0.0]},
//!       "ev": {"9s9h": 1540.0, "AhTh": 60.0, "QhJh": 210.5}
//!     },
//!     {
//!       "path": [{"kind": "check"}],
//!       "player": "ip",
//!       "actions": [{"kind": "check"}, {"kind": "bet", "amount": 500},
//!                   {"kind": "bet", "amount": 1500}],
//!       "strategy": {"KcQc": [0.3, 0.7, 0.0], "AhAd": [0.1, 0.9, 0.0]},
//!       "ev": {"KcQc": 810.2, "AhAd": 905.0}
//!     },
//!     {
//!       "path": [{"kind": "check"}, {"kind": "check"}, {"kind": "deal", "card": "9s"}],
//!       "player": "oop",
//!       "actions": [{"kind": "check"}, {"kind": "bet", "amount": 750},
//!                   {"kind": "bet", "amount": 1500}],
//!       "strategy": {"AhTh": [1.0, 0.0, 0.0], "QhJh": [0.9, 0.1, 0.0]},
//!       "ev": {"AhTh": 35.1, "QhJh": null}
//!     }
//!   ]
//! }
//! ```
//!
//! `QhJh` always bets at the root, so it never plays to the river node: a reference solver
//! writes `null` for its EV there, as this example does.
//!
//! **The header.** `version` is 2. A new key, required, comes between `spot` and `values`:
//! `complete`, `true` or `false`. It is `true` when `nodes` lists every decision node of the
//! game, every betting node on every street after every card that can fall, and `false` when
//! it lists only the first streets (below).
//!
//! **The spot.** `spot.board` holds the three or four cards of the spot's board, and
//! `spot.pot` and `spot.effective_stack` are those at the start of the spot. `spot.hands`
//! holds, for each player, the combos of its range with a positive weight that share no card
//! with the spot's board, with their range weights, as in version 1. This list never changes:
//! a hand that a later card kills stays in it.
//!
//! **The game.** Betting on each street is as in version 1, with OOP first to act on every
//! street. When a street closes with both players still in and chips behind, or with both all
//! in, the next card is dealt: any card that is not on the board and not in either player's
//! hand, each equally likely. So on a flop board, with two hands held, 45 cards can fall on the
//! turn and 44 on the river. When a player is all in and called before the river, the
//! remaining cards are dealt with no more decisions, and the hand goes to showdown on the
//! river. The rake is taken once, when the hand ends, on the final raked pot. Payoffs, EVs and
//! values count from the start of the spot, as in version 1, and average over the cards still
//! to come.
//!
//! **Paths and amounts.** A path is the list of everything public that happened from the start
//! of the spot: actions, and deals. A deal is written `{"kind": "deal", "card": "7s"}`, a rank
//! from `23456789TJQKA` and a suit from `cdhs`. It comes right after the action that closes a
//! street, the check behind or the call, and before the first action of the next street. An
//! action's `amount` is the acting player's total **for the street** once the action is taken,
//! counting only chips put in on the current street, so every street starts again from zero. A
//! call's amount is the bet or raise it calls. All-in is not a kind of its own: it is the bet
//! or raise whose street total is everything the player had left when the street began,
//! `effective_stack` less what it put in on earlier streets. On a flop spot with a pot of 550
//! and stacks of 9750, where OOP checks, IP bets 275, OOP calls, the turn is the 7s and OOP is
//! to act, the path is `[{"kind": "check"}, {"kind": "bet", "amount": 275}, {"kind": "call",
//! "amount": 275}, {"kind": "deal", "card": "7s"}]`.
//!
//! **Nodes.** `nodes` lists decision nodes only, each once, in any order. There is no node for
//! a deal, and none where no decision is left, after an all-in is called. The set listed is
//! closed under prefixes: if a node is listed, every decision node on the path to it is listed
//! too, and the root always is. A dump with `"complete": true` lists every decision node. A
//! dump with `"complete": false` lists the decision nodes of the first streets of the spot
//! only: every decision node on the first street, or on the first two streets after every
//! card. Nothing else is allowed, so a street is covered in full, for every runout, or not at
//! all. How many streets a dump holds is read from its paths: one more than the most deals on
//! any path. "Every card" means every card that can fall: a card off the board that at least
//! one pair of hands, one from each range, sharing no card and missing the board as it stands,
//! both miss. With real ranges that is every card off the board. There are no nodes after a
//! card that cannot fall, and our tree's subtree after one is not expected
//! ([`crate::import`]).
//!
//! **Strategies and EVs at a node.** `strategy` has one row for each hand of the acting player
//! that holds none of the cards dealt on the node's path, and no row for a hand that holds one;
//! on the first street that is every hand. `ev`, when given, covers the same hands. An EV is
//! `null` where it is undefined, as in version 1, and also where the solver that wrote the dump
//! has none to give: a reference solver gives none for a hand that never plays to the node
//! itself, which on later streets is a large share of the hands. A reader treats `null` as "no
//! EV here", never as zero. A hand's EV at a node is its expected payoff from the start of the
//! spot, given that play and the deals reached the node and that the player holds the hand,
//! averaged over the opponent's hands (weighted by range weight and its own probability of
//! playing to the node, among those that share no card with the hand or the board so far) and
//! over the cards still to come. The acting player's own probability of playing to the node
//! does not enter. A solver that counts EVs from the node subtracts what the acting player has
//! put in over all streets so far.
//!
//! **Values.** `values` holds each player's expected payoff per hand dealt at the start of the
//! spot, as in version 1. The two sum to the pot less the expected rake.
//!
//! # Checks
//!
//! [`SolutionDump::from_json`] checks the structure and the types, then [`SolutionDump::validate`]
//! the values, which it also does for a dump built in code. A dump is rejected for:
//!
//! - text that is not JSON, or an object with the same key twice
//! - a `format` or `version` other than the above, a missing or unknown key, or a value of the
//!   wrong type
//! - an amount, a stack, a pot or a count that is not a whole number, or is out of range
//! - a blank solver name or spot name, a number that is not finite
//! - a board that does not parse or has other than five cards in version 1, or three or four
//!   in version 2
//! - a player with no hands, a hand that does not parse, holds a board card, appears twice
//!   (in either card order) or has a weight outside `(0, 1]`
//! - no nodes, two nodes with the same path, a node with no actions or the same action twice,
//!   a fold or check with an amount, a call, bet or raise without one
//! - a strategy without a row for some hand of the acting player, or with a row for a hand
//!   that is not one, or a row with the wrong number of probabilities, a probability outside
//!   0 to 1, or probabilities that sum to more than [`ROW_TOLERANCE`] from 1
//! - EVs at some nodes and not others, or EVs whose hands are not the acting player's
//!
//! A version 2 dump is also rejected for:
//!
//! - a missing `complete`, or one that is not a boolean
//! - a deal with an amount or without a `card`, a card that does not parse, or a deal among a
//!   node's `actions`
//! - a path that breaks the betting: a deal before the street has closed, an action where the
//!   street has closed and the next card is due, a step after the hand is over or after an
//!   all-in is called, a fold or call with no bet to face, a check or bet facing one, a call
//!   whose amount is not the bet faced, a raise not above it, or a bet or raise of more than
//!   the player had left when the street began; and a node whose actions break it the same way
//! - a path that ends where no decision is left: after a fold, at a showdown, after an all-in
//!   is called, or after the action that closes a street, where the next card is dealt
//! - a deal of a card on the board, of a card dealt before on the path, or of a card that
//!   cannot fall
//! - no node at the root, or a node whose path leads through a decision node that is not
//!   listed
//! - `"complete": true` with no path that reaches the river, or `"complete": false` with a path
//!   that does, so that the dump holds every street
//! - a strategy or EV row for a hand that holds a card dealt on the node's path, or a missing
//!   row for a hand that holds none
//!
//! Each error names the field, such as `nodes[3].strategy.AsKs`, `nodes[7].path[2]` or
//! `spot.rake.cap`, and says what is wrong. Whether the dump answers a given spot, with its
//! hands and tree, and whether it lists every node of the streets it holds, is checked when it
//! is mapped onto that spot's tree ([`crate::import`]).
//!
//! # Writing
//!
//! [`SolutionDump::to_json`] writes the layout of the example: the keys in the order above,
//! nodes in our tree's order, hands in our hand order with the higher card first, and every
//! number as the shortest decimal that reads back exactly, or, for an `f32` such as one of our
//! probabilities, as the shortest decimal that reads back as that `f32`. Reading the text back
//! gives the same dump, except that such a number reads back as the `f64` nearest its decimal,
//! which rounds to the same `f32`: our strategies, stored as `f32`, come back bit for bit, and
//! every other number exactly, but for an EV or value that happens to be an `f32` value, which
//! is written as one and comes back as the same `f32`. Writing it again gives the same text. A
//! river dump is written as version 1, as it was before version 2 existed, and a flop or turn
//! dump as version 2, with `complete` after `spot`.

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write as _};
use std::fs;
use std::path::{Path, PathBuf};

use pk_cfr::PublicProfile;
use pk_core::{Card, CardSet, Combo};
use pk_game::{Action, Chips, Player, PostflopTree, Street};
use thiserror::Error;

use crate::game::PostflopGame;
use crate::node_values::node_values;
use crate::solve::Solution;

#[cfg(doc)]
use crate::import::MappedSolution;

/// The value of a dump's `format` key.
pub const DUMP_FORMAT: &str = "pk-solution-dump";

/// The version of the dump of a river spot.
pub const DUMP_VERSION: i64 = 1;

/// The version of the dump of a spot that starts on the flop or the turn.
pub const MULTI_STREET_VERSION: i64 = 2;

/// How far a hand's probabilities at a node may sum from 1. A probability written with seven
/// significant digits is within 5e-7 of its value, so this allows rows of up to 20 actions
/// written that way. Our own rows, `f32` averages, sum to within 2.1e-7
/// ([`crate::invariants`]).
pub const ROW_TOLERANCE: f64 = 1e-5;

/// The name [`SolutionDump::from_solution`] gives this solver.
pub const SOLVER_NAME: &str = "pk-postflop";

/// The largest amount a dump may hold, in chips: 1,000,000bb, as for a spot.
const MAX_CHIPS: Chips = 100_000_000;

const KEYS: [&str; 6] = ["format", "version", "solver", "spot", "values", "nodes"];
const KEYS_2: [&str; 7] = [
    "format", "version", "solver", "spot", "complete", "values", "nodes",
];
const SOLVER_KEYS: [&str; 3] = ["name", "iterations", "exploitability"];
const SPOT_KEYS: [&str; 6] = ["name", "board", "pot", "effective_stack", "rake", "hands"];
const RAKE_KEYS: [&str; 2] = ["percent", "cap"];
const PLAYER_KEYS: [&str; 2] = ["oop", "ip"];
const NODE_KEYS: [&str; 5] = ["path", "player", "actions", "strategy", "ev"];
const ACTION_KEYS: [&str; 2] = ["kind", "amount"];
const STEP_KEYS: [&str; 3] = ["kind", "amount", "card"];

#[derive(Debug, Error)]
pub enum DumpError {
    #[error("could not read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The text is not JSON, or an object holds a key twice.
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A field is missing, unknown, of the wrong type, or holds a value that is not allowed.
    #[error("`{field}` {reason}")]
    Invalid { field: String, reason: String },
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> DumpError {
    DumpError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}

/// A step of a path, as a dump names it: an action, by its kind and, for a call, bet or raise,
/// the acting player's total for the street once it is taken; or, in version 2, the deal of a
/// card. A node's actions are never deals. See the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DumpAction {
    Fold,
    Check,
    Call(Chips),
    Bet(Chips),
    Raise(Chips),
    /// The card dealt for the next street.
    Deal(Card),
}

impl DumpAction {
    /// The step's `kind`.
    pub fn kind(self) -> &'static str {
        match self {
            DumpAction::Fold => "fold",
            DumpAction::Check => "check",
            DumpAction::Call(_) => "call",
            DumpAction::Bet(_) => "bet",
            DumpAction::Raise(_) => "raise",
            DumpAction::Deal(_) => "deal",
        }
    }

    /// The action's `amount`, or `None` for a fold, a check or a deal.
    pub fn amount(self) -> Option<Chips> {
        match self {
            DumpAction::Fold | DumpAction::Check | DumpAction::Deal(_) => None,
            DumpAction::Call(amount) | DumpAction::Bet(amount) | DumpAction::Raise(amount) => {
                Some(amount)
            }
        }
    }

    /// The card a deal deals, or `None` for an action.
    pub fn card(self) -> Option<Card> {
        match self {
            DumpAction::Deal(card) => Some(card),
            _ => None,
        }
    }

    /// Whether the step is a deal rather than an action.
    pub fn is_deal(self) -> bool {
        matches!(self, DumpAction::Deal(_))
    }
}

/// `fold`, `check`, `call 500`, `bet 500`, `raise 1500` or `deal 7s`.
impl fmt::Display for DumpAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.amount(), self.card()) {
            (Some(amount), _) => write!(f, "{} {amount}", self.kind()),
            (None, Some(card)) => write!(f, "deal {card}"),
            (None, None) => f.write_str(self.kind()),
        }
    }
}

/// A path, for messages: `[]` for the root, or `[check, bet 500]`, or `[check, check, deal 7s]`.
pub struct PathText<'a>(pub &'a [DumpAction]);

impl fmt::Display for PathText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        for (i, action) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{action}")?;
        }
        f.write_str("]")
    }
}

/// The solver that made a dump's strategy.
#[derive(Debug, Clone, PartialEq)]
pub struct SolverInfo {
    pub name: String,
    pub iterations: Option<u64>,
    /// The solver's own figure for the exploitability, in chips per hand dealt.
    pub exploitability: Option<f64>,
}

/// The spot a dump answers.
#[derive(Debug, Clone, PartialEq)]
pub struct DumpSpot {
    pub name: Option<String>,
    /// The board at the start of the spot: five cards in version 1, three or four in version 2.
    pub board: CardSet,
    /// The pot at the start of the spot, in chips.
    pub pot: Chips,
    /// What each player has left to bet at the start of the spot, in chips.
    pub effective_stack: Chips,
    pub rake_percent: f64,
    /// In chips.
    pub rake_cap: Chips,
    /// Each player's hands with their range weights, indexed by [`Player::index`].
    pub hands: [Vec<(Combo, f64)>; 2],
}

/// One decision node of a dump.
#[derive(Debug, Clone, PartialEq)]
pub struct DumpNode {
    /// The actions, and in version 2 the deals, from the root to the node, oldest first.
    pub path: Vec<DumpAction>,
    pub player: Player,
    pub actions: Vec<DumpAction>,
    /// For each hand of the acting player that holds no card dealt on the path, the probability
    /// of each action, in the order of `actions`.
    pub strategy: Vec<(Combo, Vec<f64>)>,
    /// For the same hands, the EV, or `None` where there is none.
    pub ev: Option<Vec<(Combo, Option<f64>)>>,
}

impl DumpNode {
    /// The cards dealt on the node's path.
    pub fn dealt(&self) -> CardSet {
        self.path.iter().filter_map(|step| step.card()).collect()
    }

    /// The node's street, counted from the spot's first: the number of deals on its path.
    pub fn street_index(&self) -> usize {
        self.path.iter().filter(|step| step.is_deal()).count()
    }
}

/// A solution dump, as the [module documentation](self) describes it. Plain data with public
/// fields: [`SolutionDump::validate`] checks one built in code. Its version follows from its
/// board: [`SolutionDump::version`].
#[derive(Debug, Clone, PartialEq)]
pub struct SolutionDump {
    pub solver: SolverInfo,
    pub spot: DumpSpot,
    /// Whether `nodes` lists every decision node of the game. Always true for a river spot,
    /// whose version 1 has no partial form; version 2 writes it as `complete`.
    pub complete: bool,
    /// Each player's value, indexed by [`Player::index`].
    pub values: Option<[f64; 2]>,
    pub nodes: Vec<DumpNode>,
}

/// The number of streets a spot plays whose board has `cards` cards: 3 from the flop, 2 from
/// the turn and 1 on the river.
pub(crate) fn streets_from(cards: usize) -> usize {
    6usize.saturating_sub(cards)
}

/// The street dealt to a board of `cards` cards, for messages.
fn street_of(cards: usize) -> Street {
    match cards {
        0..=3 => Street::Flop,
        4 => Street::Turn,
        _ => Street::River,
    }
}

impl SolutionDump {
    pub fn load(path: impl AsRef<Path>) -> Result<SolutionDump, DumpError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| DumpError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        SolutionDump::from_json(&text)
    }

    /// Reads a dump and checks it: its structure and types, then its values as
    /// [`SolutionDump::validate`] does. The format and version are checked first, so that a
    /// dump of a later version is reported as one, and unknown keys before missing ones.
    pub fn from_json(text: &str) -> Result<SolutionDump, DumpError> {
        let value: json::Json = serde_json::from_str(text)?;
        let dump = read::dump(&value)?;
        dump.validate()?;
        Ok(dump)
    }

    /// The dump as JSON, in the layout of the [module documentation](self#writing). Reading it
    /// back gives the same dump, with each `f32` probability equal once rounded to `f32`.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        write::dump(self, &mut out).expect("writing to a string cannot fail");
        out
    }

    /// The dump of one of our solutions, complete: its spot, its hands, every decision node of
    /// its public tree with the average strategy, the EVs of
    /// [`node_values`](mod@crate::node_values), each player's value, and the iterations and
    /// exploitability of the solve. The spot's name is left out, since a solution does not
    /// hold it. A river solution gives a version 1 dump, and a flop or turn solution a version
    /// 2 dump, in which the nodes after a card that cannot fall are left out.
    ///
    /// The complete dump of a flop solution with real ranges runs to gigabytes: use
    /// [`SolutionDump::from_solution_streets`] for the first streets only.
    pub fn from_solution(solution: &Solution) -> SolutionDump {
        let streets = streets_from(solution.game().board(0).len());
        SolutionDump::from_solution_streets(solution, streets)
    }

    /// The dump of one of our solutions that holds the decision nodes of its first `streets`
    /// streets, after every card that can fall: complete if `streets` is every street the spot
    /// plays, and partial, version 2 with `"complete": false`, if it is fewer. Otherwise as
    /// [`SolutionDump::from_solution`].
    ///
    /// Panics if `streets` is 0 or more than the spot plays: 3 from the flop, 2 from the turn
    /// and 1 on the river.
    pub fn from_solution_streets(solution: &Solution, streets: usize) -> SolutionDump {
        let solver = SolverInfo {
            name: SOLVER_NAME.to_string(),
            iterations: Some(solution.iterations()),
            exploitability: Some(solution.exploitability().chips),
        };
        dump_of(
            solution.game(),
            solution.strategy(),
            solver,
            Some(solution.values()),
            streets,
        )
    }

    /// The format version the dump is written in: 1 for a river spot, 2 for a flop or turn
    /// spot.
    pub fn version(&self) -> i64 {
        if self.spot.board.len() == 5 {
            DUMP_VERSION
        } else {
            MULTI_STREET_VERSION
        }
    }

    /// The number of streets the dump holds, counted from the spot's first: one more than the
    /// most deals on any path, and 1 for a river dump.
    pub fn streets(&self) -> usize {
        1 + self
            .nodes
            .iter()
            .map(DumpNode::street_index)
            .max()
            .unwrap_or(0)
    }

    /// The node at `path`, if the dump has one.
    pub fn node(&self, path: &[DumpAction]) -> Option<&DumpNode> {
        self.nodes.iter().find(|node| node.path == path)
    }

    /// Makes the value checks of the [module documentation](self) on a dump built in code, and
    /// names the first field that fails as the text would.
    pub fn validate(&self) -> Result<(), DumpError> {
        check::dump(self)
    }
}

/// The dump of `profile` in `game`, with the EVs it gives, holding the decision nodes of the
/// first `streets` streets.
pub(crate) fn dump_of(
    game: &PostflopGame,
    profile: &PublicProfile,
    solver: SolverInfo,
    values: Option<[f64; 2]>,
    streets: usize,
) -> SolutionDump {
    let board = game.board_hands().board();
    let total = streets_from(board.len());
    assert!(
        (1..=total).contains(&streets),
        "a dump of a spot that plays {total} streets holds 1 to {total} of them, not {streets}"
    );
    let rake = game.rake();
    let hands = [Player::Oop, Player::Ip].map(|player| {
        let list = game.hands(player);
        list.combos()
            .iter()
            .copied()
            .zip(list.weights().iter().copied())
            .collect()
    });
    let spot = DumpSpot {
        name: None,
        board,
        pot: game.pot(),
        effective_stack: game.tree().spec().stack,
        rake_percent: rake.percent,
        rake_cap: rake.cap,
        hands,
    };
    let evs = node_values(game, profile);
    let nodes = decisions(game)
        .into_iter()
        .filter(|decision| !decision.dead && decision.street() < streets)
        .map(|decision| {
            let combos = game.hands(decision.player).combos();
            let num_hands = combos.len();
            let probabilities = profile
                .get(decision.node as usize)
                .expect("a profile has a strategy at every player node");
            let live = |&(_, combo): &(usize, &Combo)| !combo.intersects(decision.dealt);
            let strategy = combos
                .iter()
                .enumerate()
                .filter(live)
                .map(|(hand, &combo)| {
                    let row = (0..decision.actions.len())
                        .map(|action| f64::from(probabilities[action * num_hands + hand]))
                        .collect();
                    (combo, row)
                })
                .collect();
            let ev = evs
                .get(decision.node)
                .expect("the EVs cover every player node");
            let ev = combos
                .iter()
                .enumerate()
                .filter(live)
                .map(|(hand, &combo)| (combo, ev[hand]))
                .collect();
            DumpNode {
                path: decision.path,
                player: decision.player,
                actions: decision.actions,
                strategy,
                ev: Some(ev),
            }
        })
        .collect();
    SolutionDump {
        solver,
        spot,
        complete: streets == total,
        values,
        nodes,
    }
}

/// A decision node of our public tree, named as a dump names it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Decision {
    /// The node of the public tree.
    pub node: u32,
    pub player: Player,
    /// The actions and deals from the root.
    pub path: Vec<DumpAction>,
    pub actions: Vec<DumpAction>,
    /// The cards dealt on the way.
    pub dealt: CardSet,
    /// Whether a card dealt on the way cannot fall where it is dealt: no pair of hands that can
    /// be held there misses it. A dump has no node below such a card.
    pub dead: bool,
}

impl Decision {
    /// The node's street, counted from the spot's first: the number of cards dealt on the way.
    pub fn street(&self) -> usize {
        self.dealt.len()
    }
}

/// Every player node of a game's public tree, in the tree's pre-order, which is the order of
/// the node indices, with its path and actions as a dump writes them.
pub(crate) fn decisions(game: &PostflopGame) -> Vec<Decision> {
    let hands = [Player::Oop, Player::Ip].map(|player| game.hands(player).combos());
    let mut falls: HashMap<CardSet, CardSet> = HashMap::new();
    let mut decisions = Vec::new();
    // Each entry is a node, the path to it, the chips each player has put in on the street,
    // the cards dealt on the way and whether one of them cannot fall.
    let mut stack = vec![(
        PostflopTree::ROOT,
        Vec::new(),
        [0 as Chips; 2],
        CardSet::EMPTY,
        false,
    )];
    while let Some((node, path, put_in, dealt, dead)) = stack.pop() {
        if let Some(actor) = game.actor(node) {
            let (me, them) = (actor.index(), actor.opponent().index());
            let named: Vec<DumpAction> = game
                .actions(node)
                .iter()
                .map(|&action| dump_action(action, put_in[me], put_in[them]))
                .collect();
            // Pushed in reverse, so that the children come off the stack in action order.
            for (&action, &child) in named.iter().zip(game.children(node)).rev() {
                let mut after = put_in;
                if let Some(amount) = action.amount() {
                    after[me] = amount;
                }
                let mut path = path.clone();
                path.push(action);
                stack.push((child, path, after, dealt, dead));
            }
            decisions.push(Decision {
                node,
                player: actor,
                path,
                actions: named,
                dealt,
                dead,
            });
        } else if !game.outcomes(node).is_empty() {
            let board = game.board(node);
            let can_fall = *falls
                .entry(board)
                .or_insert_with(|| cards_that_can_fall(hands, board));
            let cards = game.outcomes(node);
            for (&card, &child) in cards.iter().zip(game.children(node)).rev() {
                let mut path = path.clone();
                path.push(DumpAction::Deal(card));
                let after = dealt | CardSet::single(card);
                stack.push((child, path, [0, 0], after, dead || !can_fall.contains(card)));
            }
        }
    }
    decisions
}

/// The cards that can fall on `board`: those off it that some pair of hands, one of each
/// player's in `hands`, sharing no card and both missing `board`, both miss.
pub(crate) fn cards_that_can_fall(hands: [&[Combo]; 2], board: CardSet) -> CardSet {
    // The cards of a pair of hands that can be held on the board and both miss `cards`.
    let pair_missing = |cards: CardSet| -> Option<CardSet> {
        let blocked = board | cards;
        for h in hands[0].iter().filter(|h| !h.intersects(blocked)) {
            let blocked = blocked | h.cards();
            if let Some(o) = hands[1].iter().find(|o| !o.intersects(blocked)) {
                return Some(h.cards() | o.cards());
            }
        }
        None
    };
    let Some(first) = pair_missing(CardSet::EMPTY) else {
        return CardSet::EMPTY;
    };
    // The first pair misses every card but its own four; each of those needs a pair of its own.
    let mut falls = CardSet::FULL - board - first;
    for card in first {
        if pair_missing(CardSet::single(card)).is_some() {
            falls.insert(card);
        }
    }
    falls
}

/// An action of our tree as a dump names it, given what the actor and the opponent have put in
/// on the street.
fn dump_action(action: Action, mine: Chips, theirs: Chips) -> DumpAction {
    match action {
        Action::Fold => DumpAction::Fold,
        Action::Check => DumpAction::Check,
        Action::Call => DumpAction::Call(theirs),
        Action::Bet { to, .. } => DumpAction::Bet(to),
        Action::Raise { to, .. } => DumpAction::Raise(to),
        Action::AllIn { to } if theirs > mine => DumpAction::Raise(to),
        Action::AllIn { to } => DumpAction::Bet(to),
    }
}

// ---------------------------------------------------------------------------------------------
// JSON values, read strictly

mod json {
    use std::collections::HashSet;
    use std::fmt;

    use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

    /// A JSON value whose objects keep their keys in order and may not repeat one.
    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum Json {
        Null,
        Bool(bool),
        /// A number written without a fraction or exponent.
        Integer(i128),
        Float(f64),
        String(String),
        Array(Vec<Json>),
        Object(Vec<(String, Json)>),
    }

    impl<'de> Deserialize<'de> for Json {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Json, D::Error> {
            deserializer.deserialize_any(JsonVisitor)
        }
    }

    struct JsonVisitor;

    impl<'de> Visitor<'de> for JsonVisitor {
        type Value = Json;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a JSON value")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Json, E> {
            Ok(Json::Bool(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Json, E> {
            Ok(Json::Integer(value.into()))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Json, E> {
            Ok(Json::Integer(value.into()))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Json, E> {
            Ok(Json::Float(value))
        }

        fn visit_str<E>(self, value: &str) -> Result<Json, E> {
            Ok(Json::String(value.to_string()))
        }

        fn visit_string<E>(self, value: String) -> Result<Json, E> {
            Ok(Json::String(value))
        }

        fn visit_unit<E>(self) -> Result<Json, E> {
            Ok(Json::Null)
        }

        fn visit_none<E>(self) -> Result<Json, E> {
            Ok(Json::Null)
        }

        fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Json, D::Error> {
            Json::deserialize(deserializer)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element()? {
                items.push(item);
            }
            Ok(Json::Array(items))
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
            let mut entries = Vec::new();
            let mut seen = HashSet::new();
            while let Some(key) = map.next_key::<String>()? {
                if !seen.insert(key.clone()) {
                    return Err(de::Error::custom(format!(
                        "the key `{key}` appears twice in one object"
                    )));
                }
                let value = map.next_value()?;
                entries.push((key, value));
            }
            Ok(Json::Object(entries))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Reading the structure and the types

mod read {
    use pk_core::{Card, CardSet, Combo};
    use pk_game::{Chips, Player};

    use super::json::Json;
    use super::*;

    /// `a`, `b` and `c`.
    pub(super) fn list(keys: &[&str]) -> String {
        let quoted: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
        match quoted.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => quoted.concat(),
        }
    }

    /// What kind of JSON value `value` is, for messages.
    fn kind(value: &Json) -> &'static str {
        match value {
            Json::Null => "null",
            Json::Bool(_) => "a boolean",
            Json::Integer(_) | Json::Float(_) => "a number",
            Json::String(_) => "a string",
            Json::Array(_) => "an array",
            Json::Object(_) => "an object",
        }
    }

    fn join(path: &str, key: &str) -> String {
        if path.is_empty() {
            key.to_string()
        } else {
            format!("{path}.{key}")
        }
    }

    fn wrong_type(field: &str, value: &Json, wanted: &str) -> DumpError {
        invalid(field, format!("is {}; it must be {wanted}", kind(value)))
    }

    /// The keys of one JSON object, named by its path, such as `spot.rake` or `nodes[3]`.
    struct Object<'a> {
        entries: &'a [(String, Json)],
        path: String,
    }

    impl<'a> Object<'a> {
        /// The object `value` at `path`, which may hold only `keys`.
        fn new(value: &'a Json, path: &str, keys: &[&str]) -> Result<Object<'a>, DumpError> {
            let Json::Object(entries) = value else {
                let wanted = format!("an object holding {}", list(keys));
                return Err(wrong_type(path, value, &wanted));
            };
            if let Some((key, _)) = entries
                .iter()
                .find(|(key, _)| !keys.contains(&key.as_str()))
            {
                let reason = if path.is_empty() {
                    format!("is not a key of a dump; its keys are {}", list(keys))
                } else {
                    format!("is not a key of `{path}`; its keys are {}", list(keys))
                };
                return Err(invalid(join(path, key), reason));
            }
            Ok(Object {
                entries,
                path: path.to_string(),
            })
        }

        fn field(&self, key: &str) -> String {
            join(&self.path, key)
        }

        /// The value of an optional key: `None` if it is left out or `null`.
        fn optional(&self, key: &str) -> Option<&'a Json> {
            self.entries
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, value)| value)
                .filter(|value| **value != Json::Null)
        }

        fn required(&self, key: &str) -> Result<&'a Json, DumpError> {
            self.optional(key)
                .ok_or_else(|| invalid(self.field(key), "is missing"))
        }
    }

    fn string<'a>(value: &'a Json, field: &str) -> Result<&'a str, DumpError> {
        match value {
            Json::String(text) => Ok(text),
            other => Err(wrong_type(field, other, "a string")),
        }
    }

    fn number(value: &Json, field: &str) -> Result<f64, DumpError> {
        match *value {
            Json::Integer(number) => Ok(number as f64),
            Json::Float(number) => Ok(number),
            ref other => Err(wrong_type(field, other, "a number")),
        }
    }

    /// A whole number from 0 to `max`, written with or without a fraction of zero.
    fn whole(value: &Json, field: &str, max: u64, what: &str) -> Result<u64, DumpError> {
        let reason = |shown: String| format!("is {shown}; it must be {what} from 0 to {max}");
        match *value {
            Json::Integer(number) => u64::try_from(number)
                .ok()
                .filter(|&n| n <= max)
                .ok_or_else(|| invalid(field, reason(number.to_string()))),
            Json::Float(number) => {
                if number.fract() != 0.0 || !(0.0..=max as f64).contains(&number) {
                    return Err(invalid(field, reason(number.to_string())));
                }
                Ok(number as u64)
            }
            ref other => Err(wrong_type(field, other, &format!("{what}, a number"))),
        }
    }

    fn chips(value: &Json, field: &str) -> Result<Chips, DumpError> {
        let chips = whole(
            value,
            field,
            u64::from(MAX_CHIPS),
            "a whole number of chips",
        )?;
        Ok(chips as Chips)
    }

    fn array<'a>(value: &'a Json, field: &str, wanted: &str) -> Result<&'a [Json], DumpError> {
        match value {
            Json::Array(items) => Ok(items),
            other => Err(wrong_type(field, other, wanted)),
        }
    }

    /// An object from hand to value, as `spot.hands.oop` or a node's `strategy`.
    fn by_hand<'a, T>(
        value: &'a Json,
        field: &str,
        wanted: &str,
        mut read: impl FnMut(&'a Json, &str) -> Result<T, DumpError>,
    ) -> Result<Vec<(Combo, T)>, DumpError> {
        let Json::Object(entries) = value else {
            return Err(wrong_type(field, value, wanted));
        };
        entries
            .iter()
            .map(|(key, value)| {
                let field = join(field, key);
                let combo: Combo = key.parse().map_err(|_| {
                    invalid(
                        &field,
                        "is not a hand: a hand is two cards, such as `AsKs` or `Td9d`",
                    )
                })?;
                Ok((combo, read(value, &field)?))
            })
            .collect()
    }

    fn player(value: &Json, field: &str) -> Result<Player, DumpError> {
        let text = string(value, field)?;
        match text.to_ascii_lowercase().as_str() {
            "oop" => Ok(Player::Oop),
            "ip" => Ok(Player::Ip),
            _ => Err(invalid(
                field,
                format!("is `{text}`; it must be `oop` or `ip`"),
            )),
        }
    }

    /// An action, in version 1 anywhere and in version 2 among a node's actions.
    fn action(value: &Json, field: &str, version: i64) -> Result<DumpAction, DumpError> {
        if version == DUMP_VERSION {
            let object = Object::new(value, field, &ACTION_KEYS)?;
            return action_in(&object, version);
        }
        let object = Object::new(value, field, &STEP_KEYS)?;
        let kind_field = object.field("kind");
        if let Some(Json::String(kind)) = object.optional("kind")
            && kind.eq_ignore_ascii_case("deal")
        {
            let reason = "is `deal`, but a deal is not an action a player takes: it is a step \
                          of a path, and a node's `actions` hold actions only";
            return Err(invalid(kind_field, reason));
        }
        if object.optional("card").is_some() {
            return Err(invalid(
                object.field("card"),
                "is given, but only a deal has a card, and a node's `actions` hold actions only",
            ));
        }
        action_in(&object, version)
    }

    /// A step of a version 2 path: an action, or a deal.
    fn step(value: &Json, field: &str) -> Result<DumpAction, DumpError> {
        let object = Object::new(value, field, &STEP_KEYS)?;
        let is_deal = matches!(
            object.optional("kind"),
            Some(Json::String(kind)) if kind.eq_ignore_ascii_case("deal")
        );
        if !is_deal {
            if object.optional("card").is_some() {
                return Err(invalid(
                    object.field("card"),
                    "is given, but only a deal has a card",
                ));
            }
            return action_in(&object, MULTI_STREET_VERSION);
        }
        if object.optional("amount").is_some() {
            return Err(invalid(
                object.field("amount"),
                "is given, but a deal has no amount; it has the `card` dealt",
            ));
        }
        let card_field = object.field("card");
        let Some(card) = object.optional("card") else {
            return Err(invalid(
                card_field,
                "is missing; a deal names the card dealt, such as `\"card\": \"7s\"`",
            ));
        };
        let text = string(card, &card_field)?;
        let card: Card = text.parse().map_err(|_| {
            invalid(
                &card_field,
                format!(
                    "is `{text}`, which is not a card: a card is a rank from `23456789TJQKA` and \
                     a suit from `cdhs`, such as `7s`"
                ),
            )
        })?;
        Ok(DumpAction::Deal(card))
    }

    /// The action an object holds, whose keys are known to be allowed.
    fn action_in(object: &Object<'_>, version: i64) -> Result<DumpAction, DumpError> {
        let kind = string(object.required("kind")?, &object.field("kind"))?;
        let amount = object.optional("amount");
        let kind = match kind.to_ascii_lowercase().as_str() {
            "fold" => DumpAction::Fold,
            "check" => DumpAction::Check,
            "call" => DumpAction::Call(0),
            "bet" => DumpAction::Bet(0),
            "raise" => DumpAction::Raise(0),
            _ => {
                let reason = if version == DUMP_VERSION {
                    format!(
                        "is `{kind}`; it must be `fold`, `check`, `call`, `bet` or `raise`. An \
                         all-in is a bet or a raise of the whole stack"
                    )
                } else {
                    format!(
                        "is `{kind}`; it must be `fold`, `check`, `call`, `bet` or `raise`, or \
                         in a path `deal`. An all-in is a bet or a raise of everything the \
                         player had left when the street began"
                    )
                };
                return Err(invalid(object.field("kind"), reason));
            }
        };
        let amount = match (kind, amount) {
            (DumpAction::Fold | DumpAction::Check, None) => return Ok(kind),
            (DumpAction::Fold | DumpAction::Check, Some(_)) => {
                let reason = format!(
                    "is given, but a {} has no amount; leave it out",
                    kind.kind()
                );
                return Err(invalid(object.field("amount"), reason));
            }
            (_, None) => {
                let reason = format!(
                    "is missing; a {} needs the acting player's total for the street once it is \
                     taken, in chips",
                    kind.kind()
                );
                return Err(invalid(object.field("amount"), reason));
            }
            (_, Some(amount)) => chips(amount, &object.field("amount"))?,
        };
        Ok(match kind {
            DumpAction::Call(_) => DumpAction::Call(amount),
            DumpAction::Bet(_) => DumpAction::Bet(amount),
            _ => DumpAction::Raise(amount),
        })
    }

    fn actions(value: &Json, field: &str, version: i64) -> Result<Vec<DumpAction>, DumpError> {
        array(value, field, "an array of actions")?
            .iter()
            .enumerate()
            .map(|(i, item)| action(item, &format!("{field}[{i}]"), version))
            .collect()
    }

    fn path(value: &Json, field: &str, version: i64) -> Result<Vec<DumpAction>, DumpError> {
        if version == DUMP_VERSION {
            return actions(value, field, version);
        }
        array(value, field, "an array of actions and deals")?
            .iter()
            .enumerate()
            .map(|(i, item)| step(item, &format!("{field}[{i}]")))
            .collect()
    }

    /// Checks the format and returns the version.
    fn check_format(top: &Json) -> Result<i64, DumpError> {
        let Json::Object(entries) = top else {
            return Err(wrong_type("", top, "an object, the dump"));
        };
        let get = |key: &str| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        match get("format") {
            Some(Json::String(format)) if format == DUMP_FORMAT => {}
            Some(other) => {
                let reason = match other {
                    Json::String(format) => format!("is `{format}`; it must be `{DUMP_FORMAT}`"),
                    other => format!("is {}; it must be the string `{DUMP_FORMAT}`", kind(other)),
                };
                return Err(invalid("format", reason));
            }
            None => {
                let reason = format!("is missing; a dump has `\"format\": \"{DUMP_FORMAT}\"`");
                return Err(invalid("format", reason));
            }
        }
        let reason = match get("version") {
            Some(Json::Integer(version))
                if *version == i128::from(DUMP_VERSION)
                    || *version == i128::from(MULTI_STREET_VERSION) =>
            {
                return Ok(*version as i64);
            }
            Some(Json::Integer(version)) => format!(
                "is {version}; this solver reads versions {DUMP_VERSION} and \
                 {MULTI_STREET_VERSION} only"
            ),
            Some(other) => format!(
                "is {}; it must be the integer {DUMP_VERSION} or {MULTI_STREET_VERSION}",
                kind(other)
            ),
            None => format!(
                "is missing; a dump has `\"version\": {DUMP_VERSION}` for a river spot, or \
                 {MULTI_STREET_VERSION} for a flop or turn spot"
            ),
        };
        Err(invalid("version", reason))
    }

    pub(super) fn dump(value: &Json) -> Result<SolutionDump, DumpError> {
        let version = check_format(value)?;
        let keys: &[&str] = if version == DUMP_VERSION {
            &KEYS
        } else {
            &KEYS_2
        };
        let top = Object::new(value, "", keys)?;

        let solver = Object::new(top.required("solver")?, "solver", &SOLVER_KEYS)?;
        let solver = SolverInfo {
            name: string(solver.required("name")?, &solver.field("name"))?.to_string(),
            iterations: solver
                .optional("iterations")
                .map(|v| whole(v, &solver.field("iterations"), u64::MAX, "a whole number"))
                .transpose()?,
            exploitability: solver
                .optional("exploitability")
                .map(|v| number(v, &solver.field("exploitability")))
                .transpose()?,
        };

        let spot = Object::new(top.required("spot")?, "spot", &SPOT_KEYS)?;
        let name = spot
            .optional("name")
            .map(|v| string(v, &spot.field("name")).map(str::to_string))
            .transpose()?;
        let board_field = spot.field("board");
        let board: CardSet = string(spot.required("board")?, &board_field)?
            .parse()
            .map_err(|err| invalid(&board_field, format!("is not a valid board: {err}")))?;
        let cards = board.len();
        if version == DUMP_VERSION && cards != 5 {
            let reason = format!(
                "has {cards} cards; version {DUMP_VERSION} holds river spots only, with a board \
                 of 5; a flop or turn spot is written as version {MULTI_STREET_VERSION}"
            );
            return Err(invalid(&board_field, reason));
        }
        if version == MULTI_STREET_VERSION && !(3..=4).contains(&cards) {
            let reason = format!(
                "has {cards} cards; version {MULTI_STREET_VERSION} holds flop and turn spots, \
                 with a board of 3 or 4; a river spot is written as version {DUMP_VERSION}"
            );
            return Err(invalid(&board_field, reason));
        }
        let pot = chips(spot.required("pot")?, &spot.field("pot"))?;
        let effective_stack = chips(
            spot.required("effective_stack")?,
            &spot.field("effective_stack"),
        )?;
        let rake = Object::new(spot.required("rake")?, "spot.rake", &RAKE_KEYS)?;
        let rake_percent = number(rake.required("percent")?, &rake.field("percent"))?;
        let rake_cap = chips(rake.required("cap")?, &rake.field("cap"))?;
        let hands = Object::new(spot.required("hands")?, "spot.hands", &PLAYER_KEYS)?;
        let mut by_player = [Vec::new(), Vec::new()];
        for (index, key) in PLAYER_KEYS.iter().enumerate() {
            let field = hands.field(key);
            by_player[index] = by_hand(
                hands.required(key)?,
                &field,
                "an object from hand to weight, such as `{\"AsKs\": 1.0}`",
                number,
            )?;
        }
        let spot = DumpSpot {
            name,
            board,
            pot,
            effective_stack,
            rake_percent,
            rake_cap,
            hands: by_player,
        };

        let complete = if version == DUMP_VERSION {
            true
        } else {
            match top.optional("complete") {
                Some(Json::Bool(complete)) => *complete,
                Some(other) => {
                    return Err(wrong_type(
                        "complete",
                        other,
                        "`true` or `false`, whether the dump lists every decision node",
                    ));
                }
                None => {
                    return Err(invalid(
                        "complete",
                        "is missing; a version 2 dump says whether it lists every decision \
                         node, with `\"complete\": true`, or those of the first streets only, \
                         with `false`",
                    ));
                }
            }
        };

        let values = top
            .optional("values")
            .map(|value| -> Result<[f64; 2], DumpError> {
                let values = Object::new(value, "values", &PLAYER_KEYS)?;
                Ok([
                    number(values.required("oop")?, &values.field("oop"))?,
                    number(values.required("ip")?, &values.field("ip"))?,
                ])
            })
            .transpose()?;

        let nodes = array(top.required("nodes")?, "nodes", "an array of nodes")?
            .iter()
            .enumerate()
            .map(|(i, item)| node(item, &format!("nodes[{i}]"), version))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(SolutionDump {
            solver,
            spot,
            complete,
            values,
            nodes,
        })
    }

    fn node(value: &Json, at: &str, version: i64) -> Result<DumpNode, DumpError> {
        let node = Object::new(value, at, &NODE_KEYS)?;
        let route = path(node.required("path")?, &node.field("path"), version)?;
        let player = player(node.required("player")?, &node.field("player"))?;
        let actions = actions(node.required("actions")?, &node.field("actions"), version)?;
        let strategy = by_hand(
            node.required("strategy")?,
            &node.field("strategy"),
            "an object from hand to an array of probabilities, one per action",
            |value, field| {
                array(value, field, "an array of probabilities, one per action")?
                    .iter()
                    .enumerate()
                    .map(|(i, p)| number(p, &format!("{field}[{i}]")))
                    .collect()
            },
        )?;
        let ev = node
            .optional("ev")
            .map(|value| {
                by_hand(
                    value,
                    &node.field("ev"),
                    "an object from hand to its EV in chips, or null",
                    |value, field| match value {
                        Json::Null => Ok(None),
                        other => number(other, field).map(Some),
                    },
                )
            })
            .transpose()?;
        Ok(DumpNode {
            path: route,
            player,
            actions,
            strategy,
            ev,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Checking the values

mod check {
    use super::*;

    fn player_key(player: Player) -> &'static str {
        match player {
            Player::Oop => "oop",
            Player::Ip => "ip",
        }
    }

    /// ` (the root)` or ` (at [check, bet 500])`, for messages about a node.
    fn at(path: &[DumpAction]) -> String {
        if path.is_empty() {
            " (the root)".to_string()
        } else {
            format!(" (at {})", PathText(path))
        }
    }

    fn finite(field: &str, number: f64) -> Result<(), DumpError> {
        if number.is_finite() {
            Ok(())
        } else {
            Err(invalid(field, format!("is {number}; it must be finite")))
        }
    }

    fn amount(field: &str, chips: Chips, positive: bool) -> Result<(), DumpError> {
        if positive && chips == 0 {
            return Err(invalid(field, "is 0; it must be more than 0"));
        }
        if chips > MAX_CHIPS {
            let reason = format!("is {chips}; an amount is at most {MAX_CHIPS} chips");
            return Err(invalid(field, reason));
        }
        Ok(())
    }

    pub(super) fn dump(dump: &SolutionDump) -> Result<(), DumpError> {
        if dump.solver.name.trim().is_empty() {
            return Err(invalid("solver.name", "is blank"));
        }
        if let Some(exploitability) = dump.solver.exploitability {
            finite("solver.exploitability", exploitability)?;
        }
        spot(&dump.spot)?;
        let version = dump.version();
        if version == DUMP_VERSION && !dump.complete {
            return Err(invalid(
                "complete",
                "is false, but the dump is of a river spot, version 1, which lists every \
                 decision node and has no partial form",
            ));
        }
        if let Some(values) = dump.values {
            for (player, value) in [Player::Oop, Player::Ip].into_iter().zip(values) {
                finite(&format!("values.{}", player_key(player)), value)?;
            }
        }
        if dump.nodes.is_empty() {
            let reason = if version == DUMP_VERSION {
                "is empty; a dump lists every decision node, the root first among them"
            } else {
                "is empty; a dump lists the root and the decision nodes below it"
            };
            return Err(invalid("nodes", reason));
        }
        let mut game = Game::new(&dump.spot);
        let mut paths: HashMap<&[DumpAction], usize> = HashMap::new();
        let with_ev = dump.nodes.iter().filter(|node| node.ev.is_some()).count();
        for (index, node) in dump.nodes.iter().enumerate() {
            let field = format!("nodes[{index}]");
            if let Some(first) = paths.insert(&node.path, index) {
                let reason = format!(
                    "is {}, the path of `nodes[{first}]` too; each node is listed once",
                    PathText(&node.path)
                );
                return Err(invalid(format!("{field}.path"), reason));
            }
            if with_ev > 0 && node.ev.is_none() {
                let reason = format!(
                    "is missing{}, but {with_ev} other nodes have EVs; give them at every node \
                     or at none",
                    at(&node.path)
                );
                return Err(invalid(format!("{field}.ev"), reason));
            }
            check_actions(node, &field)?;
            let dealt = if version == DUMP_VERSION {
                no_deals(node, &field)?;
                CardSet::EMPTY
            } else {
                game.walk(node, &field)?
            };
            check_rows(dump, node, &field, dealt, version)?;
        }
        if version == MULTI_STREET_VERSION {
            closed_under_prefixes(dump, &paths)?;
            streets(dump)?;
        }
        Ok(())
    }

    fn spot(spot: &DumpSpot) -> Result<(), DumpError> {
        if let Some(name) = &spot.name
            && name.trim().is_empty()
        {
            return Err(invalid("spot.name", "is blank; leave it out instead"));
        }
        if !(3..=5).contains(&spot.board.len()) {
            let reason = format!(
                "has {} cards; a board has 5 cards in version {DUMP_VERSION}, a river spot, and 3 \
                 or 4 in version {MULTI_STREET_VERSION}, a flop or turn spot",
                spot.board.len()
            );
            return Err(invalid("spot.board", reason));
        }
        amount("spot.pot", spot.pot, true)?;
        amount("spot.effective_stack", spot.effective_stack, true)?;
        let percent = spot.rake_percent;
        if !(percent.is_finite() && (0.0..100.0).contains(&percent)) {
            let reason = format!("is {percent}; it must be at least 0 and less than 100");
            return Err(invalid("spot.rake.percent", reason));
        }
        amount("spot.rake.cap", spot.rake_cap, false)?;
        for player in [Player::Oop, Player::Ip] {
            let field = format!("spot.hands.{}", player_key(player));
            let hands = &spot.hands[player.index()];
            if hands.is_empty() {
                return Err(invalid(field, "holds no hands"));
            }
            let mut seen = HashSet::new();
            for &(combo, weight) in hands {
                let field = format!("{field}.{combo}");
                if combo.intersects(spot.board) {
                    let reason = format!("holds a card of the board {}", spot.board);
                    return Err(invalid(field, reason));
                }
                if !seen.insert(combo) {
                    return Err(invalid(
                        field,
                        "is listed twice, perhaps with its cards in the other order",
                    ));
                }
                if !(weight.is_finite() && weight > 0.0 && weight <= 1.0) {
                    let reason = format!(
                        "has weight {weight}; a weight is more than 0 and at most 1, and a hand \
                         with none is left out"
                    );
                    return Err(invalid(field, reason));
                }
            }
        }
        Ok(())
    }

    /// A version 1 node has no deal anywhere: it is of a river spot, on which none is dealt.
    fn no_deals(node: &DumpNode, field: &str) -> Result<(), DumpError> {
        if let Some(i) = node.path.iter().position(|step| step.is_deal()) {
            return Err(invalid(
                format!("{field}.path[{i}]"),
                "is a deal, but the dump is of a river spot, version 1, on which no card is \
                 dealt",
            ));
        }
        Ok(())
    }

    /// A node offers at least one action, each once, with an amount that is in range where it
    /// has one, and no deal; and every amount on its path is in range.
    fn check_actions(node: &DumpNode, field: &str) -> Result<(), DumpError> {
        let here = at(&node.path);
        if node.actions.is_empty() {
            return Err(invalid(
                format!("{field}.actions"),
                format!("is empty{here}; a decision node offers at least one action"),
            ));
        }
        for (i, action) in node.path.iter().chain(&node.actions).enumerate() {
            if let Some(chips) = action.amount() {
                let field = if i < node.path.len() {
                    format!("{field}.path[{i}].amount")
                } else {
                    format!("{field}.actions[{}].amount", i - node.path.len())
                };
                amount(&field, chips, true)?;
            }
        }
        for (i, action) in node.actions.iter().enumerate() {
            if action.is_deal() {
                let reason = format!(
                    "is a deal{here}, but a deal is not an action a player takes: it is a step of \
                     a path"
                );
                return Err(invalid(format!("{field}.actions[{i}]"), reason));
            }
            if node.actions[..i].contains(action) {
                let reason = format!("is {action}, which comes twice{here}");
                return Err(invalid(format!("{field}.actions[{i}]"), reason));
            }
        }
        Ok(())
    }

    /// The node's `strategy` and `ev` have a row for each hand of the acting player that holds
    /// none of the cards `dealt` on its path, and each probability row is one.
    fn check_rows(
        dump: &SolutionDump,
        node: &DumpNode,
        field: &str,
        dealt: CardSet,
        version: i64,
    ) -> Result<(), DumpError> {
        let here = at(&node.path);
        let player = node.player;
        let hands: HashSet<Combo> = dump.spot.hands[player.index()]
            .iter()
            .map(|&(combo, _)| combo)
            .filter(|combo| !combo.intersects(dealt))
            .collect();
        let key = player_key(player);
        let rows_of = Rows {
            hands: &hands,
            name: format!("{player}'s hands in `spot.hands.{key}`"),
            here: here.clone(),
            dealt,
            version,
            path: &node.path,
        };
        rows_of.check(
            &format!("{field}.strategy"),
            node.strategy.iter().map(|(combo, _)| *combo),
        )?;
        let width = node.actions.len();
        for (combo, row) in &node.strategy {
            let field = format!("{field}.strategy.{combo}");
            if row.len() != width {
                let reason = format!(
                    "has {} probabilities, and the node offers {width} actions{here}",
                    row.len()
                );
                return Err(invalid(field, reason));
            }
            for (i, &p) in row.iter().enumerate() {
                if !(p.is_finite() && (0.0..=1.0).contains(&p)) {
                    let reason = format!("is {p}; a probability is from 0 to 1");
                    return Err(invalid(format!("{field}[{i}]"), reason));
                }
            }
            let sum: f64 = row.iter().sum();
            if (sum - 1.0).abs() > ROW_TOLERANCE {
                let reason = format!(
                    "sums to {sum}{here}; a hand's probabilities sum to 1, within {ROW_TOLERANCE}"
                );
                return Err(invalid(field, reason));
            }
        }
        if let Some(ev) = &node.ev {
            rows_of.check(&format!("{field}.ev"), ev.iter().map(|(combo, _)| *combo))?;
            for (combo, value) in ev {
                if let Some(value) = value {
                    finite(&format!("{field}.ev.{combo}"), *value)?;
                }
            }
        }
        Ok(())
    }

    /// The hands a node's `strategy` or `ev` must have a row for: the acting player's hands
    /// that hold no card dealt on the node's path.
    struct Rows<'a> {
        hands: &'a HashSet<Combo>,
        name: String,
        here: String,
        dealt: CardSet,
        version: i64,
        path: &'a [DumpAction],
    }

    impl Rows<'_> {
        /// Checks that the hands of a node's `strategy` or `ev` are each of `hands` once.
        fn check(&self, field: &str, combos: impl Iterator<Item = Combo>) -> Result<(), DumpError> {
            let (hands, name, here) = (self.hands, &self.name, &self.here);
            let mut seen = HashSet::new();
            for combo in combos {
                if !hands.contains(&combo) {
                    let held: Vec<Card> = (combo.cards() & self.dealt).iter().collect();
                    let reason = match held.first() {
                        Some(card) => {
                            let k = self
                                .path
                                .iter()
                                .position(|step| step.card() == Some(*card))
                                .expect("a dealt card is on the path");
                            format!(
                                "holds the {card}, dealt at `path[{k}]`{here}: a node has a row \
                                 for each of the acting player's hands that holds no card dealt \
                                 on its path, and none for a hand that holds one"
                            )
                        }
                        None => format!("is not one of {name}{here}"),
                    };
                    return Err(invalid(format!("{field}.{combo}"), reason));
                }
                if !seen.insert(combo) {
                    let reason =
                        format!("is listed twice{here}, perhaps with its cards in the other order");
                    return Err(invalid(format!("{field}.{combo}"), reason));
                }
            }
            if seen.len() < hands.len() {
                let mut missing: Vec<Combo> = hands.difference(&seen).copied().collect();
                missing.sort();
                let shown: Vec<String> = missing.iter().take(5).map(Combo::to_string).collect();
                let more = if missing.len() > 5 {
                    format!(" and {} more", missing.len() - 5)
                } else {
                    String::new()
                };
                let reason = if self.version == DUMP_VERSION {
                    format!(
                        "has no row for {}{more}, of {name}{here}; every hand has a row at every \
                         node",
                        shown.join(", ")
                    )
                } else {
                    format!(
                        "has no row for {}{more}, of {name}{here}; every hand of the acting \
                         player that holds no card dealt on the path has a row",
                        shown.join(", ")
                    )
                };
                return Err(invalid(field, reason));
            }
            Ok(())
        }
    }

    /// Where the betting stands after some steps of a path.
    #[derive(Debug, Clone, Copy)]
    enum State {
        /// The player to act has a decision.
        Open,
        /// The street closed at `path[at]`, with the call or the check behind, and the next
        /// card is due.
        Closed { at: usize },
        /// The hand is over at `path[at]`: a fold, or the close of the river, a showdown.
        Over { at: usize, fold: bool },
        /// An all-in was called at `path[at]`: the remaining cards run out to a showdown with
        /// no decision.
        AllInCalled { at: usize },
    }

    /// The game a version 2 dump's paths are walked in: the spot's stack and board, and the
    /// hands, to tell which cards can fall.
    pub(super) struct Game<'a> {
        spot: &'a DumpSpot,
        hands: [Vec<Combo>; 2],
        falls: HashMap<CardSet, CardSet>,
    }

    /// The betting on one path, step by step.
    struct Betting {
        /// The board as it stands: the spot's and the cards dealt.
        board: CardSet,
        dealt: CardSet,
        /// What each player put in on the streets before this one, and on this one.
        before: [Chips; 2],
        now: [Chips; 2],
        actor: Player,
        state: State,
    }

    impl Betting {
        fn street(&self) -> Street {
            street_of(self.board.len())
        }

        /// Whether `action` may be taken by the player to act, and why not.
        fn refuse(&self, action: DumpAction, stack: Chips) -> Option<String> {
            let (me, them) = (self.actor.index(), self.actor.opponent().index());
            let who = self.actor;
            let faced = self.now[them];
            let facing = faced > self.now[me];
            let left = stack.saturating_sub(self.before[me]);
            let street = self.street();
            let too_much = |amount: Chips, what: &str| {
                (amount > left).then(|| {
                    format!(
                        "is {what} of {amount}, more than the {left} chips {who} had left when the \
                         {street} began (`spot.effective_stack` less the {} it put in on earlier \
                         streets): an amount is the acting player's total for the street, \
                         counting only chips put in on the street",
                        self.before[me]
                    )
                })
            };
            match action {
                DumpAction::Deal(_) => None,
                DumpAction::Fold if !facing => Some(format!(
                    "is a fold, but {who} faces no bet: a fold is taken facing a bet"
                )),
                DumpAction::Check if facing => Some(format!(
                    "is a check, but {who} faces a bet of {faced}: facing a bet a player folds, \
                     calls or raises"
                )),
                DumpAction::Call(_) if !facing => Some(format!(
                    "is a call, but {who} faces no bet: with none to face a player checks or bets"
                )),
                DumpAction::Call(amount) if amount != faced => Some(format!(
                    "is a call of {amount}, but the bet faced is {faced}: a call's amount is the \
                     bet or raise it calls, the opponent's total for the street"
                )),
                DumpAction::Bet(_) if facing => Some(format!(
                    "is a bet, but {who} faces a bet of {faced}: facing a bet a player folds, \
                     calls or raises"
                )),
                DumpAction::Bet(amount) => too_much(amount, "a bet"),
                DumpAction::Raise(_) if !facing => Some(format!(
                    "is a raise, but {who} faces no bet: with none to face a player checks or \
                     bets"
                )),
                DumpAction::Raise(amount) if amount <= faced => Some(format!(
                    "is a raise to {amount}, which is not above the bet faced, {faced}: a raise's \
                     amount is the total raised to"
                )),
                DumpAction::Raise(amount) => too_much(amount, "a raise to"),
                DumpAction::Fold | DumpAction::Check | DumpAction::Call(_) => None,
            }
        }
    }

    impl Game<'_> {
        pub(super) fn new(spot: &DumpSpot) -> Game<'_> {
            let hands = spot
                .hands
                .clone()
                .map(|list| list.into_iter().map(|(combo, _)| combo).collect());
            Game {
                spot,
                hands,
                falls: HashMap::new(),
            }
        }

        fn can_fall(&mut self, board: CardSet) -> CardSet {
            let hands = [self.hands[0].as_slice(), self.hands[1].as_slice()];
            *self
                .falls
                .entry(board)
                .or_insert_with(|| cards_that_can_fall(hands, board))
        }

        /// Walks a version 2 node's path through the betting, checks its actions there, and
        /// returns the cards dealt on the way.
        pub(super) fn walk(&mut self, node: &DumpNode, field: &str) -> Result<CardSet, DumpError> {
            let stack = self.spot.effective_stack;
            let mut b = Betting {
                board: self.spot.board,
                dealt: CardSet::EMPTY,
                before: [0, 0],
                now: [0, 0],
                actor: Player::Oop,
                state: State::Open,
            };
            let here = at(&node.path);
            let last_street = |board: CardSet| board.len() >= 5;
            for (k, &step) in node.path.iter().enumerate() {
                let step_field = format!("{field}.path[{k}]");
                let refuse = |reason: String| invalid(&step_field, format!("{reason}{here}"));
                match (b.state, step) {
                    (State::Over { at, fold }, _) => {
                        let end = if fold {
                            "a fold"
                        } else {
                            "the showdown on the river"
                        };
                        return Err(refuse(format!(
                            "is {step}, after the end of the hand at `path[{at}]`, {end}: no step \
                             follows"
                        )));
                    }
                    (State::AllInCalled { at }, _) => {
                        return Err(refuse(format!(
                            "is {step}, after the call of an all-in at `path[{at}]`: the cards \
                             still to come are dealt with no more decisions, so no node lies \
                             below it and no step follows"
                        )));
                    }
                    (State::Open, DumpAction::Deal(card)) => {
                        let street = b.street();
                        let reason = if k == 0 {
                            format!(
                                "is the deal of the {card}, but a path starts with the first \
                                 action on the spot's {street}"
                            )
                        } else {
                            format!(
                                "is the deal of the {card}, but the {street} is not over: a deal \
                                 comes right after the action that closes a street, the check \
                                 behind or the call"
                            )
                        };
                        return Err(refuse(reason));
                    }
                    (State::Closed { at }, DumpAction::Deal(card)) => {
                        if self.spot.board.contains(card) {
                            return Err(refuse(format!(
                                "deals the {card}, which is on the board {}",
                                self.spot.board
                            )));
                        }
                        if b.dealt.contains(card) {
                            let first = node
                                .path
                                .iter()
                                .position(|s| s.card() == Some(card))
                                .expect("a dealt card is on the path");
                            return Err(refuse(format!(
                                "deals the {card}, which was dealt at `path[{first}]`"
                            )));
                        }
                        if !self.can_fall(b.board).contains(card) {
                            return Err(refuse(format!(
                                "deals the {card}, which cannot fall after `path[{at}]`: every \
                                 pair of hands in `spot.hands`, one of each player's, that \
                                 shares no card and misses the board {} holds it, and a dump \
                                 has no node after a card that cannot fall",
                                b.board
                            )));
                        }
                        b.board.insert(card);
                        b.dealt.insert(card);
                        b.before = [b.before[0] + b.now[0], b.before[1] + b.now[1]];
                        b.now = [0, 0];
                        b.actor = Player::Oop;
                        b.state = State::Open;
                    }
                    (State::Closed { at }, action) => {
                        let closer = closer(node.path[at]);
                        let next = street_of(b.board.len() + 1);
                        return Err(refuse(format!(
                            "is {action}, but the {} closed at `path[{at}]` with {closer}, so the \
                             next step is the deal of the {next} card",
                            b.street()
                        )));
                    }
                    (State::Open, action) => {
                        if let Some(reason) = b.refuse(action, stack) {
                            return Err(refuse(reason));
                        }
                        let (me, them) = (b.actor.index(), b.actor.opponent().index());
                        match action {
                            DumpAction::Fold => b.state = State::Over { at: k, fold: true },
                            DumpAction::Check if b.actor == Player::Ip => {
                                b.state = if last_street(b.board) {
                                    State::Over { at: k, fold: false }
                                } else {
                                    State::Closed { at: k }
                                };
                            }
                            DumpAction::Call(amount) => {
                                b.now[me] = amount;
                                let all_in = b.before[them] + b.now[them] >= stack;
                                b.state = if last_street(b.board) {
                                    State::Over { at: k, fold: false }
                                } else if all_in {
                                    State::AllInCalled { at: k }
                                } else {
                                    State::Closed { at: k }
                                };
                            }
                            DumpAction::Bet(amount) | DumpAction::Raise(amount) => {
                                b.now[me] = amount;
                            }
                            DumpAction::Check | DumpAction::Deal(_) => {}
                        }
                        b.actor = b.actor.opponent();
                    }
                }
            }
            let path_field = format!("{field}.path");
            match b.state {
                State::Open => {}
                State::Closed { at } => {
                    let closer = closer(node.path[at]);
                    let reason = format!(
                        "ends with {closer}, which closes the {}{here}: the next card is dealt \
                         there, and a dump lists no node for a deal; the next decision's path \
                         goes on with the deal",
                        b.street()
                    );
                    return Err(invalid(path_field, reason));
                }
                State::Over { fold, .. } => {
                    let end = if fold {
                        "after a fold"
                    } else {
                        "at the showdown on the river"
                    };
                    let reason = format!(
                        "ends where the hand is over, {end}{here}: a dump lists decision nodes \
                         only"
                    );
                    return Err(invalid(path_field, reason));
                }
                State::AllInCalled { .. } => {
                    let reason = format!(
                        "ends with the call of an all-in{here}: no decision is left, and the cards \
                         still to come run out to a showdown"
                    );
                    return Err(invalid(path_field, reason));
                }
            }
            for (i, &action) in node.actions.iter().enumerate() {
                if let Some(reason) = b.refuse(action, stack) {
                    let reason = reason.replacen("is ", "offers ", 1);
                    return Err(invalid(
                        format!("{field}.actions[{i}]"),
                        format!("{reason}{here}"),
                    ));
                }
            }
            Ok(b.dealt)
        }
    }

    /// `the call` or `the check behind`, for messages about the action that closed a street.
    fn closer(action: DumpAction) -> &'static str {
        match action {
            DumpAction::Call(_) => "the call",
            _ => "the check behind",
        }
    }

    /// Every decision node on the path to a listed node is listed, the root among them.
    fn closed_under_prefixes(
        dump: &SolutionDump,
        paths: &HashMap<&[DumpAction], usize>,
    ) -> Result<(), DumpError> {
        if !paths.contains_key(&[][..]) {
            return Err(invalid(
                "nodes",
                "has no node at the root, `[]`; a dump always lists the root",
            ));
        }
        for (index, node) in dump.nodes.iter().enumerate() {
            for (k, step) in node.path.iter().enumerate() {
                if step.is_deal() || paths.contains_key(&node.path[..k]) {
                    continue;
                }
                let reason = format!(
                    "leads through {}, the decision node where {step} is taken, which is not \
                     listed: the nodes of a dump are closed under prefixes, so every decision \
                     node on the path to a listed node is listed too",
                    PathText(&node.path[..k])
                );
                return Err(invalid(format!("nodes[{index}].path"), reason));
            }
        }
        Ok(())
    }

    /// `complete` agrees with the streets the paths reach.
    fn streets(dump: &SolutionDump) -> Result<(), DumpError> {
        let total = streets_from(dump.spot.board.len());
        let (index, deals) = dump
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (i, node.street_index()))
            .max_by_key(|&(i, deals)| (deals, std::cmp::Reverse(i)))
            .expect("a dump has nodes");
        let held = deals + 1;
        let first = street_of(dump.spot.board.len());
        if dump.complete && held < total {
            let reason = format!(
                "is true, but no path deals more than {deals} card{}, so the dump holds the \
                 first {held} of the {total} streets of a {first} spot: a complete dump lists \
                 every decision node, and every spot has decisions on the river, which checking \
                 through reaches",
                if deals == 1 { "" } else { "s" }
            );
            return Err(invalid("complete", reason));
        }
        if !dump.complete && held == total {
            let reason = format!(
                "is false, but `nodes[{index}].path` deals {deals} card{}, so the dump holds \
                 every street of the {first} spot: a partial dump holds the first street only, \
                 or on a flop spot the first two, and a dump of every street is complete",
                if deals == 1 { "" } else { "s" }
            );
            return Err(invalid("complete", reason));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Writing

mod write {
    use super::*;

    /// A number as the shortest JSON decimal that reads back as the same `f64`, such as `0.25`,
    /// `1.0` or `1e-7`. An `f32`, or the `f64` that the shortest decimal of an `f32` reads back
    /// as, is written as that decimal, so that a dump read back writes the same text.
    fn number(x: f64) -> String {
        debug_assert!(x.is_finite());
        let narrow = x as f32;
        let short = serde_json::to_string(&narrow).expect("a finite number is JSON");
        if f64::from(narrow) == x || short.parse::<f64>() == Ok(x) {
            short
        } else {
            serde_json::to_string(&x).expect("a finite number is JSON")
        }
    }

    fn string(text: &str) -> String {
        serde_json::to_string(text).expect("a string is JSON")
    }

    fn action(action: DumpAction) -> String {
        match (action.amount(), action.card()) {
            (Some(amount), _) => {
                format!("{{\"kind\": \"{}\", \"amount\": {amount}}}", action.kind())
            }
            (None, Some(card)) => format!("{{\"kind\": \"deal\", \"card\": \"{card}\"}}"),
            (None, None) => format!("{{\"kind\": \"{}\"}}", action.kind()),
        }
    }

    fn actions(list: &[DumpAction]) -> String {
        let items: Vec<String> = list.iter().map(|&a| action(a)).collect();
        format!("[{}]", items.join(", "))
    }

    fn player(player: Player) -> &'static str {
        match player {
            Player::Oop => "\"oop\"",
            Player::Ip => "\"ip\"",
        }
    }

    /// An object from hand to a short value, several hands to a line.
    fn short_rows<T>(
        out: &mut String,
        indent: &str,
        rows: &[(Combo, T)],
        value: impl Fn(&T) -> String,
    ) -> fmt::Result {
        const PER_LINE: usize = 8;
        out.push('{');
        for (i, (combo, item)) in rows.iter().enumerate() {
            if i % PER_LINE == 0 {
                write!(out, "\n{indent}  ")?;
            } else {
                out.push(' ');
            }
            write!(out, "\"{combo}\": {}", value(item))?;
            if i + 1 < rows.len() {
                out.push(',');
            }
        }
        write!(out, "\n{indent}}}")
    }

    pub(super) fn dump(dump: &SolutionDump, out: &mut String) -> fmt::Result {
        let version = dump.version();
        writeln!(out, "{{")?;
        writeln!(out, "  \"format\": \"{DUMP_FORMAT}\",")?;
        writeln!(out, "  \"version\": {version},")?;
        let solver = &dump.solver;
        write!(out, "  \"solver\": {{\"name\": {}", string(&solver.name))?;
        if let Some(iterations) = solver.iterations {
            write!(out, ", \"iterations\": {iterations}")?;
        }
        if let Some(exploitability) = solver.exploitability {
            write!(out, ", \"exploitability\": {}", number(exploitability))?;
        }
        writeln!(out, "}},")?;

        let spot = &dump.spot;
        writeln!(out, "  \"spot\": {{")?;
        if let Some(name) = &spot.name {
            writeln!(out, "    \"name\": {},", string(name))?;
        }
        writeln!(out, "    \"board\": \"{}\",", spot.board)?;
        writeln!(out, "    \"pot\": {},", spot.pot)?;
        writeln!(out, "    \"effective_stack\": {},", spot.effective_stack)?;
        writeln!(
            out,
            "    \"rake\": {{\"percent\": {}, \"cap\": {}}},",
            number(spot.rake_percent),
            spot.rake_cap
        )?;
        writeln!(out, "    \"hands\": {{")?;
        for (i, player) in ["oop", "ip"].iter().enumerate() {
            write!(out, "      \"{player}\": ")?;
            short_rows(out, "      ", &spot.hands[i], |&w| number(w))?;
            writeln!(out, "{}", if i == 0 { "," } else { "" })?;
        }
        writeln!(out, "    }}")?;
        writeln!(out, "  }},")?;

        if version == MULTI_STREET_VERSION {
            writeln!(out, "  \"complete\": {},", dump.complete)?;
        }

        if let Some([oop, ip]) = dump.values {
            writeln!(
                out,
                "  \"values\": {{\"oop\": {}, \"ip\": {}}},",
                number(oop),
                number(ip)
            )?;
        }

        writeln!(out, "  \"nodes\": [")?;
        for (n, node) in dump.nodes.iter().enumerate() {
            writeln!(out, "    {{")?;
            writeln!(out, "      \"path\": {},", actions(&node.path))?;
            writeln!(out, "      \"player\": {},", player(node.player))?;
            writeln!(out, "      \"actions\": {},", actions(&node.actions))?;
            write!(out, "      \"strategy\": {{")?;
            for (i, (combo, row)) in node.strategy.iter().enumerate() {
                let row: Vec<String> = row.iter().map(|&p| number(p)).collect();
                let comma = if i + 1 < node.strategy.len() { "," } else { "" };
                write!(out, "\n        \"{combo}\": [{}]{comma}", row.join(", "))?;
            }
            write!(out, "\n      }}")?;
            if let Some(ev) = &node.ev {
                write!(out, ",\n      \"ev\": ")?;
                short_rows(out, "      ", ev, |ev| match ev {
                    Some(ev) => number(*ev),
                    None => "null".to_string(),
                })?;
            }
            let comma = if n + 1 < dump.nodes.len() { "," } else { "" };
            writeln!(out, "\n    }}{comma}")?;
        }
        writeln!(out, "  ]")?;
        writeln!(out, "}}")
    }
}

#[cfg(test)]
mod tests;
