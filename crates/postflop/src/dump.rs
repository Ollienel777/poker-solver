//! Solution dumps: a river solution, ours or another solver's, in a solver-neutral JSON format,
//! so that two solvers' answers to the same spot can be compared.
//!
//! A dump is the contract between this solver and a wrapper program around a reference solver,
//! which may be written by someone who has never seen this code. So it names everything by what
//! it is, never by an index that another solver might order differently: a node by the actions
//! that lead to it, an action by its kind and amount, and a hand by its two cards. It also
//! carries the spot it answers, so that a dump of another spot, or of another tree, is caught
//! before any number in it is read.
//!
//! [`SolutionDump::from_solution`] makes the dump of one of our solutions, and
//! [`SolutionDump::to_json`] writes it. [`SolutionDump::from_json`] and [`SolutionDump::load`]
//! read a dump and check it on its own; [`MappedSolution::from_dump`] then checks it against a
//! spot and maps it onto our tree. Version 1 holds spots that start on the river.
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
//! - a board that does not parse or has other than five cards
//! - a player with no hands, a hand that does not parse, holds a board card, appears twice
//!   (in either card order) or has a weight outside `(0, 1]`
//! - no nodes, two nodes with the same path, a node with no actions or the same action twice,
//!   a fold or check with an amount, a call, bet or raise without one
//! - a strategy without a row for some hand of the acting player, or with a row for a hand
//!   that is not one, or a row with the wrong number of probabilities, a probability outside
//!   0 to 1, or probabilities that sum to more than [`ROW_TOLERANCE`] from 1
//! - EVs at some nodes and not others, or EVs whose hands are not the acting player's
//!
//! Each error names the field, such as `nodes[3].strategy.AsKs` or `spot.rake.cap`, and says
//! what is wrong. Whether the dump answers a given spot, with its hands and tree, is checked
//! when it is mapped onto that spot's tree ([`crate::import`]).
//!
//! # Writing
//!
//! [`SolutionDump::to_json`] writes the layout of the example: the keys in the order above,
//! nodes in our tree's order, hands in our hand order with the higher card first, and every
//! number as the shortest decimal that reads back exactly, or, for an `f32` such as one of our
//! probabilities, as the shortest decimal that reads back as that `f32`. Reading the text back
//! gives the same dump, except that such a probability reads back as the `f64` nearest its
//! decimal, which rounds to the same `f32`: our strategies, stored as `f32`, come back bit for
//! bit, and every other number exactly. Writing it again gives the same text.

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write as _};
use std::fs;
use std::path::{Path, PathBuf};

use pk_cfr::PublicProfile;
use pk_core::{CardSet, Combo};
use pk_game::{Action, Chips, Node, Player, PostflopTree, Street};
use thiserror::Error;

use crate::node_values::node_values;
use crate::river::RiverGame;
use crate::solve::Solution;

#[cfg(doc)]
use crate::import::MappedSolution;

/// The value of a dump's `format` key.
pub const DUMP_FORMAT: &str = "pk-solution-dump";

/// The version of the dump format that this module reads and writes.
pub const DUMP_VERSION: i64 = 1;

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
const SOLVER_KEYS: [&str; 3] = ["name", "iterations", "exploitability"];
const SPOT_KEYS: [&str; 6] = ["name", "board", "pot", "effective_stack", "rake", "hands"];
const RAKE_KEYS: [&str; 2] = ["percent", "cap"];
const PLAYER_KEYS: [&str; 2] = ["oop", "ip"];
const NODE_KEYS: [&str; 5] = ["path", "player", "actions", "strategy", "ev"];
const ACTION_KEYS: [&str; 2] = ["kind", "amount"];

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

/// An action, as a dump names it: by its kind and, for a call, bet or raise, the acting
/// player's total for the street once it is taken. See the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DumpAction {
    Fold,
    Check,
    Call(Chips),
    Bet(Chips),
    Raise(Chips),
}

impl DumpAction {
    /// The action's `kind`.
    pub fn kind(self) -> &'static str {
        match self {
            DumpAction::Fold => "fold",
            DumpAction::Check => "check",
            DumpAction::Call(_) => "call",
            DumpAction::Bet(_) => "bet",
            DumpAction::Raise(_) => "raise",
        }
    }

    /// The action's `amount`, or `None` for a fold or a check.
    pub fn amount(self) -> Option<Chips> {
        match self {
            DumpAction::Fold | DumpAction::Check => None,
            DumpAction::Call(amount) | DumpAction::Bet(amount) | DumpAction::Raise(amount) => {
                Some(amount)
            }
        }
    }
}

/// `fold`, `check`, `call 500`, `bet 500` or `raise 1500`.
impl fmt::Display for DumpAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.amount() {
            Some(amount) => write!(f, "{} {amount}", self.kind()),
            None => f.write_str(self.kind()),
        }
    }
}

/// A path, for messages: `[]` for the root, or `[check, bet 500]`.
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
    pub board: CardSet,
    /// The pot at the start of the river, in chips.
    pub pot: Chips,
    /// What each player has left to bet at the start of the river, in chips.
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
    /// The actions from the root to the node, oldest first.
    pub path: Vec<DumpAction>,
    pub player: Player,
    pub actions: Vec<DumpAction>,
    /// For each hand of the acting player, the probability of each action, in the order of
    /// `actions`.
    pub strategy: Vec<(Combo, Vec<f64>)>,
    /// For each hand of the acting player, its EV, or `None` where it is undefined.
    pub ev: Option<Vec<(Combo, Option<f64>)>>,
}

/// A solution dump, as the [module documentation](self) describes it. Plain data with public
/// fields: [`SolutionDump::validate`] checks one built in code.
#[derive(Debug, Clone, PartialEq)]
pub struct SolutionDump {
    pub solver: SolverInfo,
    pub spot: DumpSpot,
    /// Each player's value, indexed by [`Player::index`].
    pub values: Option<[f64; 2]>,
    pub nodes: Vec<DumpNode>,
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

    /// The dump of one of our solutions: its spot, its hands, every decision node of its tree
    /// with the average strategy, the EVs of [`node_values`](mod@crate::node_values), each
    /// player's value, and the iterations and exploitability of the solve. The spot's name is
    /// left out, since a solution does not hold it.
    pub fn from_solution(solution: &Solution) -> SolutionDump {
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
        )
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

/// The dump of `profile` in `game`, with the EVs it gives.
pub(crate) fn dump_of(
    game: &RiverGame,
    profile: &PublicProfile,
    solver: SolverInfo,
    values: Option<[f64; 2]>,
) -> SolutionDump {
    let board = game.board_hands().board();
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
    let nodes = decision_nodes(game.tree())
        .into_iter()
        .map(|decision| {
            let combos = game.hands(decision.player).combos();
            let num_hands = combos.len();
            let probabilities = profile
                .get(decision.node as usize)
                .expect("a profile has a strategy at every player node");
            let strategy = combos
                .iter()
                .enumerate()
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
            let ev = combos.iter().copied().zip(ev.iter().copied()).collect();
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
        values,
        nodes,
    }
}

/// A decision node of our tree, named as a dump names it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Decision {
    pub node: u32,
    pub player: Player,
    pub path: Vec<DumpAction>,
    pub actions: Vec<DumpAction>,
}

/// Every player node of a river tree, in the tree's pre-order, with its path and actions as a
/// dump writes them.
///
/// Panics if the tree has a node off the river.
pub(crate) fn decision_nodes(tree: &PostflopTree) -> Vec<Decision> {
    let mut decisions = Vec::new();
    // Each entry is a node, the path to it, and the chips each player has put in so far.
    let mut stack = vec![(PostflopTree::ROOT, Vec::new(), [0 as Chips; 2])];
    while let Some((node, path, put_in)) = stack.pop() {
        let Node::Player {
            street,
            actor,
            actions,
            children,
        } = tree.node(node)
        else {
            continue;
        };
        assert_eq!(*street, Street::River, "a dump holds river trees only");
        let (me, them) = (actor.index(), actor.opponent().index());
        let named: Vec<DumpAction> = actions
            .iter()
            .map(|&action| dump_action(action, put_in[me], put_in[them]))
            .collect();
        // Pushed in reverse, so that the children come off the stack in action order.
        for (&action, &child) in named.iter().zip(children).rev() {
            let mut after = put_in;
            if let Some(amount) = action.amount() {
                after[me] = amount;
            }
            let mut path = path.clone();
            path.push(action);
            stack.push((child, path, after));
        }
        decisions.push(Decision {
            node,
            player: *actor,
            path,
            actions: named,
        });
    }
    decisions
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
    use pk_core::{CardSet, Combo};
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

    fn action(value: &Json, field: &str) -> Result<DumpAction, DumpError> {
        let object = Object::new(value, field, &ACTION_KEYS)?;
        let kind = string(object.required("kind")?, &object.field("kind"))?;
        let amount = object.optional("amount");
        let kinds = "`fold`, `check`, `call`, `bet` or `raise`";
        let kind = match kind.to_ascii_lowercase().as_str() {
            "fold" => DumpAction::Fold,
            "check" => DumpAction::Check,
            "call" => DumpAction::Call(0),
            "bet" => DumpAction::Bet(0),
            "raise" => DumpAction::Raise(0),
            _ => {
                let reason = format!(
                    "is `{kind}`; it must be {kinds}. An all-in is a bet or a raise of the whole \
                     stack"
                );
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

    fn actions(value: &Json, field: &str) -> Result<Vec<DumpAction>, DumpError> {
        array(value, field, "an array of actions")?
            .iter()
            .enumerate()
            .map(|(i, item)| action(item, &format!("{field}[{i}]")))
            .collect()
    }

    fn check_format(top: &Json) -> Result<(), DumpError> {
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
            Some(Json::Integer(version)) if *version == i128::from(DUMP_VERSION) => return Ok(()),
            Some(Json::Integer(version)) => {
                format!("is {version}; this solver reads version {DUMP_VERSION} only")
            }
            Some(other) => format!("is {}; it must be the integer {DUMP_VERSION}", kind(other)),
            None => format!("is missing; a dump has `\"version\": {DUMP_VERSION}`"),
        };
        Err(invalid("version", reason))
    }

    pub(super) fn dump(value: &Json) -> Result<SolutionDump, DumpError> {
        check_format(value)?;
        let top = Object::new(value, "", &KEYS)?;

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
            .map(|(i, item)| node(item, &format!("nodes[{i}]")))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(SolutionDump {
            solver,
            spot,
            values,
            nodes,
        })
    }

    fn node(value: &Json, path: &str) -> Result<DumpNode, DumpError> {
        let node = Object::new(value, path, &NODE_KEYS)?;
        let route = actions(node.required("path")?, &node.field("path"))?;
        let player = player(node.required("player")?, &node.field("player"))?;
        let actions = actions(node.required("actions")?, &node.field("actions"))?;
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
        if let Some(values) = dump.values {
            for (player, value) in [Player::Oop, Player::Ip].into_iter().zip(values) {
                finite(&format!("values.{}", player_key(player)), value)?;
            }
        }
        if dump.nodes.is_empty() {
            return Err(invalid(
                "nodes",
                "is empty; a dump lists every decision node, the root first among them",
            ));
        }
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
            check_node(dump, node, &field)?;
        }
        Ok(())
    }

    fn spot(spot: &DumpSpot) -> Result<(), DumpError> {
        if let Some(name) = &spot.name
            && name.trim().is_empty()
        {
            return Err(invalid("spot.name", "is blank; leave it out instead"));
        }
        if spot.board.len() != 5 {
            let reason = format!(
                "has {} cards; version {DUMP_VERSION} holds river spots only, with a board of 5",
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

    fn check_node(dump: &SolutionDump, node: &DumpNode, field: &str) -> Result<(), DumpError> {
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
            if node.actions[..i].contains(action) {
                let reason = format!("is {action}, which comes twice{here}");
                return Err(invalid(format!("{field}.actions[{i}]"), reason));
            }
        }
        let player = node.player;
        let hands: HashSet<Combo> = dump.spot.hands[player.index()]
            .iter()
            .map(|&(combo, _)| combo)
            .collect();
        let key = player_key(player);
        let name = format!("{player}'s hands in `spot.hands.{key}`");
        rows(
            &format!("{field}.strategy"),
            &hands,
            &name,
            &here,
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
            rows(
                &format!("{field}.ev"),
                &hands,
                &name,
                &here,
                ev.iter().map(|(combo, _)| *combo),
            )?;
            for (combo, value) in ev {
                if let Some(value) = value {
                    finite(&format!("{field}.ev.{combo}"), *value)?;
                }
            }
        }
        Ok(())
    }

    /// Checks that the hands of a node's `strategy` or `ev` are each of `hands` once.
    fn rows(
        field: &str,
        hands: &HashSet<Combo>,
        name: &str,
        here: &str,
        combos: impl Iterator<Item = Combo>,
    ) -> Result<(), DumpError> {
        let mut seen = HashSet::new();
        for combo in combos {
            if !hands.contains(&combo) {
                let reason = format!("is not one of {name}{here}");
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
            let reason = format!(
                "has no row for {}{more}, of {name}{here}; every hand has a row at every node",
                shown.join(", ")
            );
            return Err(invalid(field, reason));
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
        match action.amount() {
            Some(amount) => format!("{{\"kind\": \"{}\", \"amount\": {amount}}}", action.kind()),
            None => format!("{{\"kind\": \"{}\"}}", action.kind()),
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
        writeln!(out, "{{")?;
        writeln!(out, "  \"format\": \"{DUMP_FORMAT}\",")?;
        writeln!(out, "  \"version\": {DUMP_VERSION},")?;
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
