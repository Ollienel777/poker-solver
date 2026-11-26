//! The JSON form of what `inspect` shows: a solution file as its header describes it
//! ([`FileJson`]) and one node of its solution ([`NodeJson`]), built from [`crate::view`]'s
//! structures and holding the same numbers. `inspect --json` prints it and `solver serve` answers
//! with it, so it is the contract the viewer's page is written against; ARCHITECTURE.md's
//! `serve` section lists every field.
//!
//! Amounts are integer chips, 1bb = 100, as everywhere in the solver: the pot, the stacks, what
//! each player has put in and what an action adds. Values and EVs are chips that need not be
//! whole, from the start of the spot. Frequencies, shares and probabilities are fractions
//! from 0 to 1, not percents; exploitabilities carry both a percent of the pot and chips. A
//! number that is undefined, such as the EV of a combo whose opponent never plays to the node,
//! is `null`. Players are `"oop"` and `"ip"`, and streets `"flop"`, `"turn"` and `"river"`, as
//! in a solution dump. Cards, combos and classes are written as everywhere else: `8h`, `AhKh`,
//! `AKs`.
//!
//! The types derive [`Serialize`], as the solution file's header does; field order is the
//! declaration order.

use pk_core::{Card, HandClass};
use pk_game::{Action, Outcome, Player, Street, StreetSizing};
use pk_postflop::{Exploitability, Payoff, Spot, StopReason};
use serde::Serialize;

use crate::view::{
    ActionView, Breakdown, ClassRow, Decision, FileView, Missing, Next, NodeKind, NodeView,
    RiverSolve, Step,
};

/// What the viewer asks for at a node, and what `inspect --json` prints: the file and the node.
#[derive(Debug, Clone, Serialize)]
pub struct NodeAnswer {
    pub file: FileJson,
    pub node: NodeJson,
}

impl NodeAnswer {
    pub fn new(file: &FileView, node: &NodeView) -> NodeAnswer {
        NodeAnswer {
            file: FileJson::new(file),
            node: NodeJson::new(node),
        }
    }
}

/// A solution file, from its header alone.
#[derive(Debug, Clone, Serialize)]
pub struct FileJson {
    /// The file's name, without its folder.
    pub name: String,
    /// Its length, in bytes.
    pub bytes: u64,
    /// The program that wrote it and its version, such as `pk-postflop 0.1.0`.
    pub written_by: String,
    pub spot: SpotJson,
    /// The streets the file holds, from the spot's first.
    pub streets: Vec<&'static str>,
    /// Whether it holds every street, the river included. A file that does not has its river
    /// solved again when a node on it is asked for.
    pub complete: bool,
    /// The public tree's nodes, and those where a player acts.
    pub nodes: u32,
    pub player_nodes: u32,
    /// Each player's hands, the combos of its range that the board leaves, OOP's first.
    pub hands: [u32; 2],
    /// How the solve stopped: `"target"`, `"iteration_cap"` or `"time_cap"`.
    pub stop: &'static str,
    pub iterations: u64,
    /// The exploitability of the strategy as the file stores it, each probability in 16 bits.
    pub exploitability: ExploitabilityJson,
    /// The exploitability the solve measured, before the strategy was stored.
    pub solve_exploitability: ExploitabilityJson,
    /// Each player's value, in chips per hand dealt from the start of the spot, OOP's first.
    pub values: [f64; 2],
}

impl FileJson {
    pub fn new(file: &FileView) -> FileJson {
        let header = &file.header;
        FileJson {
            name: file.path.file_name().map_or_else(
                || file.path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
            bytes: file.bytes,
            written_by: header.written_by(),
            spot: SpotJson::new(&header.spot),
            streets: header.streets.iter().map(|&s| street(s)).collect(),
            complete: header.is_complete(),
            nodes: header.nodes,
            player_nodes: header.player_nodes,
            hands: header.hands,
            stop: stop(header.stop),
            iterations: header.iterations,
            exploitability: ExploitabilityJson::new(&header.exploitability),
            solve_exploitability: ExploitabilityJson::new(&header.solve_exploitability),
            values: header.exploitability.values,
        }
    }
}

/// An exploitability: as a percent of the pot at the start of the spot, and in chips per hand
/// dealt.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ExploitabilityJson {
    pub percent_of_pot: f64,
    pub chips: f64,
}

impl ExploitabilityJson {
    pub fn new(exploitability: &Exploitability) -> ExploitabilityJson {
        ExploitabilityJson {
            percent_of_pot: exploitability.percent_of_pot,
            chips: exploitability.chips,
        }
    }
}

/// The spot a file was solved for. A file knows no positions: the players are OOP and IP, and
/// a spot made from a range file has the formation in its name, such as
/// `BTN_vs_BB_SRP_Ks7d2c`.
#[derive(Debug, Clone, Serialize)]
pub struct SpotJson {
    pub name: String,
    pub description: String,
    /// The board the spot starts on, its cards from the highest.
    pub board: Vec<String>,
    /// The street the spot starts on.
    pub street: &'static str,
    /// The pot when the spot starts, and what each player has behind then, in chips.
    pub pot: u32,
    pub stack: u32,
    /// The rake, as a percent of the pot, and its cap in chips.
    pub rake_percent: f64,
    pub rake_cap: u32,
    pub tree: TreeJson,
}

impl SpotJson {
    pub fn new(spot: &Spot) -> SpotJson {
        let tree = &spot.tree;
        let streets = [
            (Street::Flop, &tree.flop),
            (Street::Turn, &tree.turn),
            (Street::River, &tree.river),
        ]
        .into_iter()
        .filter(|&(s, _)| s >= spot.street())
        .filter_map(|(s, sizing)| sizing.as_ref().map(|sizing| SizingJson::new(s, sizing)))
        .collect();
        SpotJson {
            name: spot.name.clone(),
            description: spot.description.clone(),
            board: cards_from_highest(spot.board.iter()),
            street: street(spot.street()),
            pot: spot.pot,
            stack: spot.effective_stack,
            rake_percent: spot.rake_percent,
            rake_cap: spot.rake_cap,
            tree: TreeJson {
                max_raises: tree.max_raises,
                allin_threshold: tree.allin_threshold,
                streets,
            },
        }
    }
}

/// The betting tree's sizing.
#[derive(Debug, Clone, Serialize)]
pub struct TreeJson {
    /// The most raises a street may have after its opening bet.
    pub max_raises: u32,
    /// A bet or raise that would put in more than this fraction of the stack goes all-in.
    pub allin_threshold: f64,
    /// The sizing of each street the spot plays, from its first.
    pub streets: Vec<SizingJson>,
}

/// One street's sizing.
#[derive(Debug, Clone, Serialize)]
pub struct SizingJson {
    pub street: &'static str,
    /// Bets as fractions of the pot, such as 0.5.
    pub bets: Vec<f64>,
    /// Raises as multiples of the bet faced, such as 3.
    pub raises: Vec<f64>,
    pub allin: bool,
}

impl SizingJson {
    fn new(s: Street, sizing: &StreetSizing) -> SizingJson {
        SizingJson {
            street: street(s),
            bets: sizing.bets.clone(),
            raises: sizing.raises.clone(),
            allin: sizing.allin,
        }
    }
}

/// One node of the solution.
#[derive(Debug, Clone, Serialize)]
pub struct NodeJson {
    /// The node's path, its tokens separated by one space: `""` at the root.
    pub path: String,
    /// The steps from the root to the node, one per token of the path.
    pub line: Vec<StepJson>,
    /// The node's street: a deal is on the street it deals.
    pub street: &'static str,
    /// The board at the node: the spot's cards from the highest, then each card dealt on the
    /// way, in the order dealt.
    pub board: Vec<String>,
    /// The pot at the node, in chips: the spot's, and all both players have put in since.
    pub pot: u32,
    /// What each player has put in since the spot started, and has left behind, in chips, OOP's
    /// first.
    pub put_in: [u32; 2],
    pub behind: [u32; 2],
    /// The probability that a hand dealt plays to the node.
    pub reached: f64,
    /// Each player's range at the node in combos, each hand's range weight times its own reach
    /// summed, OOP's first.
    pub combos: [f64; 2],
    /// What the node is, `"decision"`, `"deal"` or `"end"`, with the fields of that kind.
    #[serde(flatten)]
    pub kind: KindJson,
    /// The river solved again to show the node, when the file leaves its river out; `null`
    /// otherwise.
    pub river: Option<RiverJson>,
}

impl NodeJson {
    pub fn new(node: &NodeView) -> NodeJson {
        let line: Vec<StepJson> = node
            .line
            .iter()
            .enumerate()
            .map(|(index, step)| StepJson::new(&node.line[..=index], step))
            .collect();
        // The dealt cards are on the path; the rest of the board is the spot's.
        let dealt: Vec<Card> = node
            .line
            .iter()
            .filter(|step| step.actor.is_none())
            .map(|step| step.token.parse().expect("a deal's token is a card"))
            .collect();
        let mut board = cards_from_highest(node.board.iter().filter(|card| !dealt.contains(card)));
        board.extend(dealt.iter().map(Card::to_string));
        NodeJson {
            path: node.path.clone(),
            line,
            street: street(node.street),
            board,
            pot: node.pot,
            put_in: node.put_in,
            behind: node.behind,
            reached: node.reached,
            combos: node.combos,
            kind: KindJson::new(node),
            river: node.river.as_ref().map(RiverJson::new),
        }
    }
}

/// One step of the path to a node.
#[derive(Debug, Clone, Serialize)]
pub struct StepJson {
    /// Its token, such as `x`, `b50` or `8h`.
    pub token: String,
    /// The path up to and including it: the node the step leads to.
    pub path: String,
    /// The player who acts, or `null` for a card dealt.
    pub actor: Option<&'static str>,
    /// The street it is on: for a card, the street it deals.
    pub street: &'static str,
}

impl StepJson {
    fn new(through: &[Step], step: &Step) -> StepJson {
        StepJson {
            token: step.token.clone(),
            path: through
                .iter()
                .map(|step| step.token.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            actor: step.actor.map(player),
            street: street(step.street),
        }
    }
}

/// What a node is, with what each kind has.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KindJson {
    /// A player acts.
    Decision {
        actor: &'static str,
        /// The actions, in the tree's order, which every list of frequencies follows.
        actions: Vec<ActionJson>,
        /// All 169 hand classes in grid order: `13 * row + column`, rows and columns from the
        /// ace to the deuce, pairs on the diagonal, suited hands above it and offsuit below.
        classes: Vec<ClassJson>,
        /// The combos of the hand asked for, or `null` if none was.
        hand: Option<BreakdownJson>,
    },
    /// A card is dealt, for the node's street: any of `cards`, in the order of their index.
    Deal { cards: Vec<CardJson> },
    /// The hand is over.
    End {
        /// `"fold"` or `"showdown"`.
        outcome: &'static str,
        /// After a fold, the player who wins the pot; `null` at a showdown.
        winner: Option<&'static str>,
        payoff: PayoffJson,
    },
}

impl KindJson {
    fn new(node: &NodeView) -> KindJson {
        match &node.kind {
            NodeKind::Decision(decision) => decision_json(node, decision),
            NodeKind::Deal { cards, .. } => KindJson::Deal {
                cards: cards
                    .iter()
                    .map(|card| CardJson {
                        card: card.to_string(),
                        path: joined(&node.path, &card.to_string()),
                    })
                    .collect(),
            },
            NodeKind::End { outcome, payoff } => {
                let (outcome, winner) = outcome_json(*outcome);
                KindJson::End {
                    outcome,
                    winner,
                    payoff: PayoffJson::new(payoff),
                }
            }
        }
    }
}

fn decision_json(node: &NodeView, decision: &Decision) -> KindJson {
    let mut classes: Vec<ClassJson> = HandClass::all().map(ClassJson::outside).collect();
    for row in &decision.classes {
        classes[row.class.index()] = ClassJson::new(row);
    }
    KindJson::Decision {
        actor: player(decision.actor),
        actions: decision
            .actions
            .iter()
            .map(|action| ActionJson::new(&node.path, action))
            .collect(),
        classes,
        hand: decision.breakdown.as_ref().map(BreakdownJson::new),
    }
}

/// A card a deal can deal.
#[derive(Debug, Clone, Serialize)]
pub struct CardJson {
    /// The card, which is its token.
    pub card: String,
    /// The path of the node it leads to.
    pub path: String,
}

/// One action at a decision.
#[derive(Debug, Clone, Serialize)]
pub struct ActionJson {
    /// Its token, such as `x`, `c`, `f`, `b50`, `r3` or `a`.
    pub token: String,
    /// The path of the node it leads to: the node's path with the token after it.
    pub path: String,
    /// `"fold"`, `"check"`, `"call"`, `"bet"`, `"raise"` or `"allin"`.
    pub action: &'static str,
    /// The chips it puts in: the bet, the raise or the call's difference; 0 for a check or a
    /// fold.
    pub adds: u32,
    /// The acting player's chips on the street once it is taken.
    pub to: u32,
    /// How often it is taken once play reaches the node, card removal by the opponent's range
    /// included; `null` if play never reaches the node.
    pub frequency: Option<f64>,
    /// Its frequency over the acting player's range alone, which the classes' frequencies
    /// average; `null` if the player never plays to the node.
    pub range_frequency: Option<f64>,
    pub next: NextJson,
}

impl ActionJson {
    fn new(path: &str, action: &ActionView) -> ActionJson {
        ActionJson {
            token: action.token.clone(),
            path: joined(path, &action.token),
            action: match action.action {
                Action::Fold => "fold",
                Action::Check => "check",
                Action::Call => "call",
                Action::Bet { .. } => "bet",
                Action::Raise { .. } => "raise",
                Action::AllIn { .. } => "allin",
            },
            adds: action.adds,
            to: action.to,
            frequency: action.frequency,
            range_frequency: action.range_frequency,
            next: NextJson::new(action.next),
        }
    }
}

/// What an action leads to.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NextJson {
    /// A decision of `player`.
    Acts { player: &'static str },
    /// A deal of the street `street`.
    Deals { street: &'static str },
    /// The end of the hand, as [`KindJson::End`] has it.
    Ends {
        outcome: &'static str,
        winner: Option<&'static str>,
    },
}

impl NextJson {
    fn new(next: Next) -> NextJson {
        match next {
            Next::Acts(who) => NextJson::Acts {
                player: player(who),
            },
            Next::Deals { street: s, .. } => NextJson::Deals { street: street(s) },
            Next::Ends(outcome) => {
                let (outcome, winner) = outcome_json(outcome);
                NextJson::Ends { outcome, winner }
            }
        }
    }
}

/// One hand class at a decision.
#[derive(Debug, Clone, Serialize)]
pub struct ClassJson {
    /// Its name, such as `AA`, `AKs` or `AKo`.
    pub class: String,
    /// Whether it is in the acting player's range at the node: whether one of its combos is
    /// there with a weight above 0. The other fields of a class outside it are 0 and `null`.
    pub in_range: bool,
    /// Its combos in the range: their range weights times their own reaches, summed.
    pub combos: f64,
    /// Its share of the range, as a range view weighs it.
    pub share: f64,
    /// Each action's frequency over its combos in the range, in the order of `actions`.
    pub frequencies: Option<Vec<f64>>,
    /// Its EV, in chips from the start of the spot: its combos' EVs averaged by their weights,
    /// over the combos that have one; `null` if none has.
    pub ev: Option<f64>,
}

impl ClassJson {
    fn new(row: &ClassRow) -> ClassJson {
        ClassJson {
            class: row.class.to_string(),
            in_range: true,
            combos: row.combos,
            share: row.share,
            frequencies: Some(row.frequencies.clone()),
            ev: row.ev,
        }
    }

    /// A class that is not in the range.
    fn outside(class: HandClass) -> ClassJson {
        ClassJson {
            class: class.to_string(),
            in_range: false,
            combos: 0.0,
            share: 0.0,
            frequencies: None,
            ev: None,
        }
    }
}

/// The combos of a class or combo asked for at a decision.
#[derive(Debug, Clone, Serialize)]
pub struct BreakdownJson {
    /// The hand asked for, as a class or a combo: `AKs`, `AhKh`.
    pub query: String,
    /// A row for each of its combos among the acting player's hands at the node, in the
    /// class's order.
    pub rows: Vec<ComboJson>,
    /// Its other combos, and why each has no row.
    pub missing: Vec<MissingJson>,
}

impl BreakdownJson {
    fn new(breakdown: &Breakdown) -> BreakdownJson {
        BreakdownJson {
            query: match breakdown.query {
                crate::view::HandQuery::Class(class) => class.to_string(),
                crate::view::HandQuery::Combo(combo) => combo.to_string(),
            },
            rows: breakdown
                .rows
                .iter()
                .map(|row| ComboJson {
                    combo: row.combo.to_string(),
                    weight: row.weight,
                    strategy: row.strategy.clone(),
                    ev: row.ev,
                })
                .collect(),
            missing: breakdown
                .missing
                .iter()
                .map(|&(combo, why)| MissingJson {
                    combo: combo.to_string(),
                    reason: match why {
                        Missing::OnBoard => "on_board",
                        Missing::NotInRange => "not_in_range",
                    },
                })
                .collect(),
        }
    }
}

/// One combo of the acting player's at a decision.
#[derive(Debug, Clone, Serialize)]
pub struct ComboJson {
    pub combo: String,
    /// Its weight in the range at the node: its range weight times its own reach.
    pub weight: f64,
    /// The probability of each action, in the order of `actions`.
    pub strategy: Vec<f64>,
    /// Its EV, in chips from the start of the spot; `null` where it is undefined.
    pub ev: Option<f64>,
}

/// A combo asked for that has no row.
#[derive(Debug, Clone, Serialize)]
pub struct MissingJson {
    pub combo: String,
    /// `"on_board"`, if it holds a card of the board, or `"not_in_range"`.
    pub reason: &'static str,
}

/// A terminal's payoffs, in chips from the start of the spot, the rake taken.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PayoffJson {
    /// After a fold: each player's payoff, OOP's first, whatever the cards.
    Fold { values: [f64; 2] },
    /// At a showdown: the payoff of the better hand, the worse, and each of a split.
    Showdown { win: f64, lose: f64, tie: f64 },
}

impl PayoffJson {
    fn new(payoff: &Payoff) -> PayoffJson {
        match *payoff {
            Payoff::Fold(values) => PayoffJson::Fold { values },
            Payoff::Showdown(showdown) => PayoffJson::Showdown {
                win: showdown.win,
                lose: showdown.lose,
                tie: showdown.tie,
            },
        }
    }
}

/// A river the file leaves out, solved again to show the node.
#[derive(Debug, Clone, Serialize)]
pub struct RiverJson {
    /// The path of the river's first decision, where the river spot starts.
    pub start: String,
    /// The pot at the start of the river, in chips.
    pub pot: u32,
    /// How the river's solve stopped, as a file's `stop`.
    pub stop: &'static str,
    pub iterations: u64,
    /// Its exploitability in the river spot's terms: per hand dealt at the start of the river,
    /// and as a percent of its pot.
    pub exploitability: ExploitabilityJson,
    /// What it was solved to, as a percent of the river's pot.
    pub target: f64,
    /// The time the solve took, in seconds.
    pub seconds: f64,
}

impl RiverJson {
    fn new(river: &RiverSolve) -> RiverJson {
        RiverJson {
            start: river.start.clone(),
            pot: river.pot,
            stop: stop(river.stop),
            iterations: river.iterations,
            exploitability: ExploitabilityJson::new(&river.exploitability),
            target: river.target,
            seconds: river.elapsed.as_secs_f64(),
        }
    }
}

fn player(player: Player) -> &'static str {
    match player {
        Player::Oop => "oop",
        Player::Ip => "ip",
    }
}

fn street(street: Street) -> &'static str {
    street.name()
}

pub fn stop(stop: StopReason) -> &'static str {
    match stop {
        StopReason::Target => "target",
        StopReason::IterationCap => "iteration_cap",
        StopReason::TimeCap => "time_cap",
    }
}

/// An outcome's name and, after a fold, its winner.
fn outcome_json(outcome: Outcome) -> (&'static str, Option<&'static str>) {
    match outcome {
        Outcome::Fold { winner } => ("fold", Some(player(winner))),
        Outcome::Showdown => ("showdown", None),
    }
}

/// Cards from the highest, as a board is written.
fn cards_from_highest(cards: impl Iterator<Item = Card>) -> Vec<String> {
    let mut cards: Vec<Card> = cards.collect();
    cards.sort_by_key(|card| std::cmp::Reverse(card.index()));
    cards.iter().map(Card::to_string).collect()
}

/// `path` with `token` after it.
fn joined(path: &str, token: &str) -> String {
    if path.is_empty() {
        token.to_string()
    } else {
        format!("{path} {token}")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use pk_postflop::{SaveOptions, SavedSolution, SolveOptions, save, solve};
    use serde_json::{Value, json};

    use super::*;
    use crate::view::{self, HandQuery};

    /// `turn_small`, solved and saved in the folder `name` under the system's temporary folder,
    /// holding the streets up to `last_street`.
    fn turn_file(name: &str, last_street: Street) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("pk-cli-json-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let spot = Spot::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../postflop/tests/data/turn_small.toml"),
        )
        .unwrap();
        let solution = solve(&spot, &SolveOptions::default(), |_| {}).unwrap();
        let file = folder.join("turn.pksol");
        save(&solution, &file, &SaveOptions { last_street }).unwrap();
        file
    }

    /// The JSON of the node at `path` of `file`, read back, and the view it was made from.
    fn both(file: &Path, path: &str, query: Option<HandQuery>) -> (NodeView, Value) {
        let saved = SavedSolution::load(file).unwrap();
        let node = view::find(&saved, path).unwrap();
        let shown = view::node_view(&saved, file, node, query).unwrap();
        let info = FileView::read(file).unwrap();
        let text = serde_json::to_string(&NodeAnswer::new(&info, &shown)).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        (shown, value["node"].clone())
    }

    fn f64s(value: &Value) -> Vec<f64> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect()
    }

    fn option(value: &Value) -> Option<f64> {
        (!value.is_null()).then(|| value.as_f64().unwrap())
    }

    /// Checks that `json` holds every number of `view`, exactly.
    fn check(view: &NodeView, json: &Value) {
        assert_eq!(json["path"], view.path.as_str());
        let line = json["line"].as_array().unwrap();
        assert_eq!(line.len(), view.line.len());
        let tokens: Vec<&str> = view.line.iter().map(|s| s.token.as_str()).collect();
        for (index, (step, shown)) in view.line.iter().zip(line).enumerate() {
            assert_eq!(shown["token"], step.token.as_str());
            assert_eq!(shown["path"], tokens[..=index].join(" ").as_str());
            assert_eq!(shown["actor"].as_str(), step.actor.map(player));
            assert_eq!(shown["street"], street(step.street));
        }
        assert_eq!(json["street"], street(view.street));
        let board: Vec<Card> = json["board"]
            .as_array()
            .unwrap()
            .iter()
            .map(|card| card.as_str().unwrap().parse().unwrap())
            .collect();
        assert_eq!(board.len(), view.board.len());
        assert_eq!(
            board.iter().copied().collect::<pk_core::CardSet>(),
            view.board
        );
        assert_eq!(json["pot"], view.pot);
        assert_eq!(json["put_in"], json!(view.put_in));
        assert_eq!(json["behind"], json!(view.behind));
        assert_eq!(json["reached"].as_f64(), Some(view.reached));
        assert_eq!(f64s(&json["combos"]), view.combos);
        match &view.kind {
            NodeKind::Decision(decision) => {
                assert_eq!(json["kind"], "decision");
                assert_eq!(json["actor"], player(decision.actor));
                let actions = json["actions"].as_array().unwrap();
                assert_eq!(actions.len(), decision.actions.len());
                for (action, shown) in decision.actions.iter().zip(actions) {
                    assert_eq!(shown["token"], action.token.as_str());
                    assert_eq!(shown["path"], joined(&view.path, &action.token).as_str());
                    assert_eq!(shown["adds"], action.adds);
                    assert_eq!(shown["to"], action.to);
                    assert_eq!(option(&shown["frequency"]), action.frequency);
                    assert_eq!(option(&shown["range_frequency"]), action.range_frequency);
                }
                let classes = json["classes"].as_array().unwrap();
                assert_eq!(classes.len(), pk_core::NUM_CLASSES);
                for (index, shown) in classes.iter().enumerate() {
                    let class = HandClass::from_index(index);
                    assert_eq!(shown["class"], class.to_string().as_str());
                    match decision.classes.iter().find(|row| row.class == class) {
                        Some(row) => {
                            assert_eq!(shown["in_range"], true);
                            assert_eq!(shown["combos"].as_f64(), Some(row.combos));
                            assert_eq!(shown["share"].as_f64(), Some(row.share));
                            assert_eq!(f64s(&shown["frequencies"]), row.frequencies);
                            assert_eq!(option(&shown["ev"]), row.ev);
                        }
                        None => {
                            assert_eq!(shown["in_range"], false);
                            assert!(shown["frequencies"].is_null() && shown["ev"].is_null());
                        }
                    }
                }
                match &decision.breakdown {
                    None => assert!(json["hand"].is_null()),
                    Some(breakdown) => {
                        let rows = json["hand"]["rows"].as_array().unwrap();
                        assert_eq!(rows.len(), breakdown.rows.len());
                        for (row, shown) in breakdown.rows.iter().zip(rows) {
                            assert_eq!(shown["combo"], row.combo.to_string().as_str());
                            assert_eq!(shown["weight"].as_f64(), Some(row.weight));
                            assert_eq!(f64s(&shown["strategy"]), row.strategy);
                            assert_eq!(option(&shown["ev"]), row.ev);
                        }
                        let missing = json["hand"]["missing"].as_array().unwrap();
                        assert_eq!(missing.len(), breakdown.missing.len());
                        assert_eq!(rows.len() + missing.len(), breakdown.query.combos().len());
                    }
                }
            }
            NodeKind::Deal { cards, .. } => {
                assert_eq!(json["kind"], "deal");
                let shown = json["cards"].as_array().unwrap();
                assert_eq!(shown.len(), cards.len());
                for (card, shown) in cards.iter().zip(shown) {
                    assert_eq!(shown["card"], card.to_string().as_str());
                    assert_eq!(
                        shown["path"],
                        joined(&view.path, &card.to_string()).as_str()
                    );
                }
            }
            NodeKind::End { payoff, .. } => {
                assert_eq!(json["kind"], "end");
                match payoff {
                    Payoff::Fold(values) => {
                        assert_eq!(json["payoff"]["kind"], "fold");
                        assert_eq!(f64s(&json["payoff"]["values"]), values);
                    }
                    Payoff::Showdown(showdown) => {
                        assert_eq!(json["payoff"]["kind"], "showdown");
                        assert_eq!(json["payoff"]["win"].as_f64(), Some(showdown.win));
                        assert_eq!(json["payoff"]["lose"].as_f64(), Some(showdown.lose));
                        assert_eq!(json["payoff"]["tie"].as_f64(), Some(showdown.tie));
                    }
                }
            }
        }
        match &view.river {
            None => assert!(json["river"].is_null()),
            Some(river) => {
                assert_eq!(json["river"]["start"], river.start.as_str());
                assert_eq!(json["river"]["pot"], river.pot);
                assert_eq!(json["river"]["iterations"], river.iterations);
                assert_eq!(
                    json["river"]["exploitability"]["percent_of_pot"].as_f64(),
                    Some(river.exploitability.percent_of_pot)
                );
                assert_eq!(json["river"]["target"].as_f64(), Some(river.target));
            }
        }
    }

    /// Every kind of node, on a file that holds every street and on a river solved again from
    /// a file without it, holds in its JSON every number of its view.
    #[test]
    fn the_json_of_a_node_holds_the_numbers_of_its_view() {
        let whole = turn_file("whole", Street::River);
        let partial = turn_file("partial", Street::Turn);
        let class = Some(HandQuery::Class("AKs".parse().unwrap()));
        let combo = Some(HandQuery::Combo("AhTh".parse().unwrap()));
        for (file, path, query) in [
            (&whole, "", None),
            (&whole, "b50", class),
            (&whole, "x x 3c b75", combo),
            (&whole, "x x", None),
            (&whole, "b50 f", None),
            (&whole, "x x 3c x x", None),
            (&partial, "x x 3c", combo),
            (&partial, "b50 c 8c a", class),
        ] {
            let (shown, json) = both(file, path, query);
            check(&shown, &json);
        }
        let (shown, json) = both(&partial, "x x 3c", None);
        assert!(shown.river.is_some() && !json["river"].is_null());

        // A deal's cards with their paths, and the board in the order it was dealt.
        let (_, json) = both(&whole, "x x", None);
        assert_eq!(json["cards"][0], json!({"card": "2c", "path": "x x 2c"}));
        let (_, json) = both(&whole, "x x 3c", None);
        assert_eq!(json["board"], json!(["Kh", "9c", "7d", "2s", "3c"]));
        assert_eq!(json["line"][2]["actor"], Value::Null);

        // The file, from its header.
        let info = FileView::read(&partial).unwrap();
        let file = serde_json::to_value(FileJson::new(&info)).unwrap();
        assert_eq!(file["name"], "turn.pksol");
        assert_eq!(file["bytes"], info.bytes);
        assert_eq!(file["streets"], json!(["turn"]));
        assert_eq!(file["complete"], false);
        assert_eq!(file["spot"]["board"], json!(["Kh", "9c", "7d", "2s"]));
        assert_eq!(file["spot"]["pot"], info.header.spot.pot);
        assert_eq!(file["spot"]["stack"], info.header.spot.effective_stack);
        assert_eq!(file["spot"]["tree"]["streets"][0]["street"], "turn");
        assert_eq!(file["iterations"], info.header.iterations);
        assert_eq!(
            f64s(&file["values"]),
            info.header.exploitability.values.to_vec()
        );
        for file in [whole, partial] {
            fs::remove_dir_all(file.parent().unwrap()).unwrap();
        }
    }
}
