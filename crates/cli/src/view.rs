//! What `inspect` shows of a solution, as data: the file it was read from ([`FileView`]) and one
//! node of its tree ([`NodeView`]), with the acting player's actions and range there by hand
//! class and, for one class or combo, by combo. Nothing here prints: `commands::inspect` prints
//! these, and a viewer can show the same.
//!
//! Every number is one of the library's read-outs of the solution: the frequencies of
//! [`Solution::action_frequencies`] and [`Solution::range_view_frequencies`], the classes of
//! [`Solution::range_view_classes`], the reaches of [`Solution::reach`] and
//! [`Solution::reach_probability`], and the EVs the file holds ([`SavedSolution::ev`]). This
//! module only sums them over a class's combos.
//!
//! A node on a river that the file leaves out is shown from that river solved again
//! ([`RiverSolve`]): the river that starts at the first decision after the river card on the
//! node's path, as [`SavedSolution::river_spot`] gives it, solved to [`RIVER_TARGET`] of its pot.
//! Its EVs, which count from the start of the river, are moved to the start of the spot with
//! [`RiverSpot::put_in`](pk_postflop::RiverSpot::put_in), and its reaches are scaled by the
//! probability that play reaches the river, so that every node reads the same way.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use pk_core::{Card, CardSet, Combo, HandClass};
use pk_game::{Action, Chips, Node, Outcome, Player, Street};
use pk_postflop::solution_file::Header;
use pk_postflop::{
    Exploitability, NodePathError, NodePathErrorKind, NodeValues, Payoff, PostflopGame,
    SavedSolution, Solution, SolutionFileError, SolveOptions, StopReason, node_values, solve,
};

/// The target a river left out of a file is solved again to, as a percent of the river's pot:
/// 0.01%, thirty times tighter than `solve-postflop`'s default of 0.3%. On the rivers of
/// `BTN_vs_BB_SRP` on Ks7d2c under `configs/trees/one_bet.toml`, it takes 100 to 240
/// iterations and 10 to 20 ms (DECISIONS P76).
pub const RIVER_TARGET: f64 = 0.01;

/// The most iterations a river left out of a file is solved again for: about four times what
/// the target took on the rivers measured.
pub const RIVER_ITERATIONS: u64 = 1_000;

/// A solution file, as its header describes it.
#[derive(Debug, Clone)]
pub struct FileView {
    pub path: PathBuf,
    /// The file's length, in bytes.
    pub bytes: u64,
    pub header: Header,
}

impl FileView {
    /// Reads the header of the solution file at `path`, and its length.
    pub fn read(path: &Path) -> Result<FileView, SolutionFileError> {
        let header = pk_postflop::read_header(path)?;
        let bytes = fs::metadata(path)
            .map_err(|source| SolutionFileError::Io {
                action: "read",
                path: path.to_path_buf(),
                source,
            })?
            .len();
        Ok(FileView {
            path: path.to_path_buf(),
            bytes,
            header,
        })
    }
}

/// The hands to break down at a node, combo by combo: a class, or one combo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandQuery {
    Class(HandClass),
    Combo(Combo),
}

impl HandQuery {
    /// Reads a class, such as `AKs`, `77` or `T9o`, or a combo, such as `AhKh`, in any case.
    pub fn parse(text: &str) -> Option<HandQuery> {
        let text = text.trim();
        if text.chars().count() == 4 {
            text.parse().ok().map(HandQuery::Combo)
        } else {
            text.parse().ok().map(HandQuery::Class)
        }
    }

    /// The combos it names: the class's, in the class's order, or the one combo.
    pub fn combos(self) -> Vec<Combo> {
        match self {
            HandQuery::Class(class) => class.combos().collect(),
            HandQuery::Combo(combo) => vec![combo],
        }
    }
}

/// One node of a solution's tree, as `inspect` shows it.
#[derive(Debug, Clone)]
pub struct NodeView {
    /// The node's path, its tokens separated by one space: empty at the root.
    pub path: String,
    /// The steps of the path, from the root: an action or a card dealt each.
    pub line: Vec<Step>,
    /// The node's street: a chance node is on the street it deals.
    pub street: Street,
    /// The board at the node: the spot's and every card dealt on the way.
    pub board: CardSet,
    /// The pot at the node, in chips: the spot's, and all both players have put in since.
    pub pot: Chips,
    /// The chips each player has put in since the spot started, indexed by [`Player::index`].
    pub put_in: [Chips; 2],
    /// The chips each player has left behind.
    pub behind: [Chips; 2],
    /// The probability that a hand dealt plays to the node.
    pub reached: f64,
    /// Each player's range at the node, in combos: each hand's range weight times its own
    /// reach of the node, summed.
    pub combos: [f64; 2],
    pub kind: NodeKind,
    /// The river solved again to show the node, when the file leaves its river out.
    pub river: Option<RiverSolve>,
}

/// One step of the path to a node: an action, or a card dealt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Its token in a node path, such as `b50` or `8h`.
    pub token: String,
    /// The player who takes the action; `None` for a card dealt.
    pub actor: Option<Player>,
    /// The street it is on: for a card, the street it deals.
    pub street: Street,
}

/// What a node is.
#[derive(Debug, Clone)]
pub enum NodeKind {
    /// A player acts.
    Decision(Decision),
    /// A card is dealt for `street`: any of `cards`.
    Deal { street: Street, cards: Vec<Card> },
    /// The hand ends: by `outcome`, with `payoff` from the start of the spot.
    End { outcome: Outcome, payoff: Payoff },
}

/// A node where a player acts.
#[derive(Debug, Clone)]
pub struct Decision {
    pub actor: Player,
    /// The node's actions, in the tree's order.
    pub actions: Vec<ActionView>,
    /// The hand classes of the acting player's range at the node, in grid order. Empty if the
    /// player never plays to the node.
    pub classes: Vec<ClassRow>,
    /// The combos of the class or combo asked for, if one was.
    pub breakdown: Option<Breakdown>,
}

/// One action at a decision.
#[derive(Debug, Clone)]
pub struct ActionView {
    pub action: Action,
    /// Its token in a node path, such as `b50`.
    pub token: String,
    /// The chips it puts in: the bet, the raise or the call's difference; 0 for a check or a
    /// fold.
    pub adds: Chips,
    /// The acting player's total on the street once it is taken.
    pub to: Chips,
    /// How often it is taken once play reaches the node, card removal by the opponent's range
    /// included ([`Solution::action_frequencies`]). `None` if play never reaches the node.
    pub frequency: Option<f64>,
    /// Its frequency over the acting player's range alone, each combo by its range weight
    /// times its own reach ([`Solution::range_view_frequencies`]): what the grids average.
    /// `None` if the player never plays to the node.
    pub range_frequency: Option<f64>,
    /// What follows it.
    pub next: Next,
}

/// What an action leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Acts(Player),
    /// A chance node that deals `street`; `example` is the first card it can deal.
    Deals {
        street: Street,
        example: Card,
    },
    Ends(Outcome),
}

/// One hand class in the acting player's range at a node.
#[derive(Debug, Clone)]
pub struct ClassRow {
    pub class: HandClass,
    /// Its combos in the range: their range weights times their own reaches, summed.
    pub combos: f64,
    /// Its share of the range, as a range view weighs it: [`pk_postflop::ClassView::share`].
    pub share: f64,
    /// Each action's frequency over its combos in the range, in the node's order:
    /// [`pk_postflop::ClassView::frequencies`].
    pub frequencies: Vec<f64>,
    /// Its EV, in chips from the start of the spot: its combos' EVs averaged by their weights
    /// in the range, over the combos that have one. `None` if none has.
    pub ev: Option<f64>,
}

/// The combos of a class or combo asked for at a decision.
#[derive(Debug, Clone)]
pub struct Breakdown {
    pub query: HandQuery,
    /// A row for each of its combos that is among the acting player's hands at the node.
    pub rows: Vec<ComboRow>,
    /// Its other combos, and why each has no row.
    pub missing: Vec<(Combo, Missing)>,
}

/// Why a combo asked for has no row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// It holds a card of the board at the node.
    OnBoard,
    /// It is not in the acting player's range: not in the spot's, or, on a river solved again,
    /// not in the range that reaches the river.
    NotInRange,
}

/// One combo of the acting player's at a decision.
#[derive(Debug, Clone)]
pub struct ComboRow {
    pub combo: Combo,
    /// Its weight in the range at the node: its range weight times its own reach.
    pub weight: f64,
    /// The probability of each action, in the node's order.
    pub strategy: Vec<f64>,
    /// Its EV, in chips from the start of the spot; `None` where it is undefined.
    pub ev: Option<f64>,
}

/// A river that the file leaves out, solved again to show a node on it.
#[derive(Debug, Clone)]
pub struct RiverSolve {
    /// The path of the river's first decision, where the river spot starts.
    pub start: String,
    /// The pot at the start of the river, in chips.
    pub pot: Chips,
    pub stop: StopReason,
    pub iterations: u64,
    /// The river solve's exploitability, in the river spot's terms: per hand dealt at the
    /// start of the river, and as a percent of its pot.
    pub exploitability: Exploitability,
    /// What it was solved to, as a percent of the river's pot.
    pub target: f64,
    /// The time the solve took, building its game included.
    pub elapsed: Duration,
}

/// The node of `saved`'s game that `path` leads to. A path that does not lead to a node is an
/// error that says where it went wrong and what could follow there.
pub fn find(saved: &SavedSolution, path: &str) -> Result<u32> {
    let game = saved.solution().game();
    // Action tokens are lower case, and cards read in either case.
    let path = path.to_lowercase();
    game.find(&path)
        .map_err(|error| path_error(game, &path, &error))
}

/// `--node`'s error for `path`, which `game` refused with `error`.
fn path_error(game: &PostflopGame, path: &str, error: &NodePathError) -> anyhow::Error {
    let shown = path.split_whitespace().collect::<Vec<_>>().join(" ");
    match &error.kind {
        NodePathErrorKind::NotACard { .. } | NodePathErrorKind::OnBoard { .. } => {
            let at = game
                .find(&error.before.join(" "))
                .expect("the tokens before the one that failed lead to a node");
            let cards: Vec<String> = game.outcomes(at).iter().map(Card::to_string).collect();
            anyhow!(
                "--node `{shown}`: {error}; the cards that can fall there are the {} off the board \
                 {}: {}",
                cards.len(),
                game.board(at),
                cards.join(" ")
            )
        }
        NodePathErrorKind::NoAction { .. } | NodePathErrorKind::HandOver { .. } => {
            anyhow!("--node `{shown}`: {error}")
        }
    }
}

/// How the strategy at and above a node is read: from the file, or from a river solved again.
struct Reader<'a> {
    /// The solution the strategy is in: the file's, or the river's.
    solution: &'a Solution,
    /// The node in that solution's game.
    node: u32,
    evs: Evs<'a>,
    /// The probability that play reaches the root of that solution's game.
    reach_scale: f64,
}

/// Where a reader's EVs come from.
enum Evs<'a> {
    /// The file's, from the start of the spot.
    File(&'a SavedSolution),
    /// The river solve's, which count from the start of the river, with what each player had
    /// put in before it.
    River {
        values: NodeValues,
        put_in: [Chips; 2],
    },
}

impl Reader<'_> {
    /// The EV, in chips from the start of the spot, of the acting player's hand `hand` at the
    /// reader's node, which is a player node.
    fn ev(&self, actor: Player, hand: usize) -> Option<f64> {
        match &self.evs {
            Evs::File(saved) => {
                let ev = saved.ev(self.node).expect("the file holds the node's EVs")[hand];
                (!ev.is_nan()).then_some(f64::from(ev))
            }
            Evs::River { values, put_in } => {
                let ev = values
                    .get(self.node)
                    .expect("the river's EVs cover its nodes")[hand]?;
                Some(ev - f64::from(put_in[actor.index()]))
            }
        }
    }
}

/// The view of `node` of `saved`'s game, with the combos of `query` if the node is a decision.
/// A node on a river the file leaves out is shown from that river solved again; one on a turn
/// the file leaves out is an error that says what the file holds.
pub fn node_view(
    saved: &SavedSolution,
    file: &Path,
    node: u32,
    query: Option<HandQuery>,
) -> Result<NodeView> {
    let solution = saved.solution();
    let game = solution.game();
    let path = game.node_path(node);
    // The first player node at or above the node whose strategy the solution does not hold.
    let steps = game.path(node);
    let unheld = steps
        .iter()
        .map(|&(at, _)| at)
        .chain([node])
        .find(|&at| game.actor(at).is_some() && !solution.holds(at));
    let Some(start) = unheld else {
        let reader = Reader {
            solution,
            node,
            evs: Evs::File(saved),
            reach_scale: 1.0,
        };
        return Ok(view(game, node, &path, &reader, query, None));
    };
    if game.street(start) != Street::River {
        bail!(
            "--node `{path}` is after the {}, the last street `{}` holds, and only a river can \
             be solved again; solve the spot again with solve-postflop --last-street turn or \
             river, from the spot that `solver inspect {} --spot` prints",
            saved.header().last_street(),
            file.display(),
            file.display()
        );
    }

    // The river that starts at its first decision on the node's path, solved again.
    let started = Instant::now();
    let river = saved.river_spot(start).map_err(|error| {
        anyhow!(
            "--node `{path}` is on a river that `{}` leaves out, and that river cannot be solved \
             again: {error}",
            file.display()
        )
    })?;
    let options = SolveOptions {
        target: Some(RIVER_TARGET),
        max_iterations: RIVER_ITERATIONS,
        ..SolveOptions::default()
    };
    let solved = solve(&river.spot, &options, |_| {}).map_err(|error| {
        anyhow!(
            "--node `{path}`: the river after `{}`, which `{}` leaves out, could not be solved \
             again: {error}",
            game.node_path(start),
            file.display()
        )
    })?;
    let elapsed = started.elapsed();
    let start_path = game.node_path(start);
    let skip = start_path.split_whitespace().count();
    let rest: Vec<&str> = path.split_whitespace().skip(skip).collect();
    let river_node = solved.game().find(&rest.join(" ")).map_err(|error| {
        anyhow!(
            "--node `{path}`: the river solved again after `{start_path}` has no node `{}`: \
             {error}",
            rest.join(" ")
        )
    })?;
    let values = node_values(solved.game(), solved.strategy());
    let reader = Reader {
        solution: &solved,
        node: river_node,
        evs: Evs::River {
            values,
            put_in: river.put_in,
        },
        reach_scale: solution.reach_probability(start),
    };
    let resolved = RiverSolve {
        start: start_path,
        pot: river.spot.pot,
        stop: solved.stop(),
        iterations: solved.iterations(),
        exploitability: *solved.exploitability(),
        target: RIVER_TARGET,
        elapsed,
    };
    Ok(view(game, node, &path, &reader, query, Some(resolved)))
}

/// The view of `node` of `game`, the full game, whose strategy `reader` reads.
fn view(
    game: &PostflopGame,
    node: u32,
    path: &str,
    reader: &Reader<'_>,
    query: Option<HandQuery>,
    river: Option<RiverSolve>,
) -> NodeView {
    let spot_stack = game.tree().spec().stack;
    let (put_in, street_put_in) = chips_at(game, node);
    let solution = reader.solution;
    let reach = solution.reach(reader.node);
    let combos = [Player::Oop, Player::Ip].map(|player| {
        let weights = solution.hands(player).weights();
        weights
            .iter()
            .zip(&reach[player.index()])
            .map(|(w, r)| w * r)
            .sum()
    });
    let board = game.board(node);
    let kind = match game.tree().node(game.tree_node(node)) {
        Node::Player { actor, .. } => NodeKind::Decision(decision(
            game,
            node,
            street_put_in,
            reader,
            &reach[actor.index()],
            query,
        )),
        Node::Chance { street, .. } => NodeKind::Deal {
            street: *street,
            cards: game.outcomes(node).to_vec(),
        },
        Node::Terminal { outcome, .. } => NodeKind::End {
            outcome: *outcome,
            payoff: game.payoff(node).expect("a terminal has its payoffs"),
        },
    };
    let line = game
        .path(node)
        .into_iter()
        .map(|(at, index)| {
            let actor = game.actor(at);
            let token = match actor {
                Some(_) => game.actions(at)[index].to_string(),
                None => game.outcomes(at)[index].to_string(),
            };
            Step {
                token,
                actor,
                street: game.street(at),
            }
        })
        .collect();
    NodeView {
        path: path.to_string(),
        line,
        street: game.street(node),
        board,
        pot: game.pot() + put_in[0] + put_in[1],
        put_in,
        behind: put_in.map(|chips| spot_stack - chips),
        reached: reader.reach_scale * solution.reach_probability(reader.node),
        combos,
        kind,
        river,
    }
}

/// The decision at the player node `node` of `game`, with `street_put_in` each player's chips
/// on the street so far and `own_reach` the acting player's reach by hand in `reader`'s
/// solution.
fn decision(
    game: &PostflopGame,
    node: u32,
    street_put_in: [Chips; 2],
    reader: &Reader<'_>,
    own_reach: &[f64],
    query: Option<HandQuery>,
) -> Decision {
    let actor = game.actor(node).expect("a decision is at a player node");
    let board = game.board(node);
    let solution = reader.solution;
    let at = reader.node;
    let frequencies = solution.action_frequencies(at);
    let range_frequencies = solution.range_view_frequencies(at);
    let (mine, theirs) = (
        street_put_in[actor.index()],
        street_put_in[actor.opponent().index()],
    );
    let actions = game
        .actions(node)
        .iter()
        .zip(game.children(node))
        .enumerate()
        .map(|(index, (&action, &child))| {
            let to = match action {
                Action::Fold | Action::Check => mine,
                Action::Call => theirs,
                Action::Bet { to, .. } | Action::Raise { to, .. } | Action::AllIn { to } => to,
            };
            ActionView {
                action,
                token: action.to_string(),
                adds: to - mine,
                to,
                frequency: frequencies.as_ref().map(|f| f[index]),
                range_frequency: range_frequencies.as_ref().map(|f| f[index]),
                next: next_of(game, child),
            }
        })
        .collect();

    let weights = solution.hands(actor).weights();
    let weight = |hand: usize| weights[hand] * own_reach[hand];
    let classes = solution
        .range_view_classes(at)
        .unwrap_or_default()
        .into_iter()
        .map(|class| {
            let combos = class.hands.iter().map(|&h| weight(h)).sum();
            let (mut total, mut sum) = (0.0, 0.0);
            for &hand in &class.hands {
                let w = weight(hand);
                if let (true, Some(ev)) = (w > 0.0, reader.ev(actor, hand)) {
                    total += w;
                    sum += w * ev;
                }
            }
            ClassRow {
                class: class.class,
                combos,
                share: class.share,
                frequencies: class.frequencies,
                ev: (total > 0.0).then(|| sum / total),
            }
        })
        .collect();

    let breakdown = query.map(|query| {
        let hands = solution.hands(actor);
        let mut rows = Vec::new();
        let mut missing = Vec::new();
        for combo in query.combos() {
            if combo.intersects(board) {
                missing.push((combo, Missing::OnBoard));
                continue;
            }
            let Some(hand) = hands.position(combo) else {
                missing.push((combo, Missing::NotInRange));
                continue;
            };
            let strategy = solution
                .hand_strategy(at, hand)
                .expect("the solution holds the node")
                .into_iter()
                .map(f64::from)
                .collect();
            rows.push(ComboRow {
                combo,
                weight: weight(hand),
                strategy,
                ev: reader.ev(actor, hand),
            });
        }
        Breakdown {
            query,
            rows,
            missing,
        }
    });

    Decision {
        actor,
        actions,
        classes,
        breakdown,
    }
}

/// What the node `child` of `game` is, as what follows an action.
fn next_of(game: &PostflopGame, child: u32) -> Next {
    match game.tree().node(game.tree_node(child)) {
        Node::Player { actor, .. } => Next::Acts(*actor),
        Node::Chance { street, .. } => Next::Deals {
            street: *street,
            example: game.outcomes(child)[0],
        },
        Node::Terminal { outcome, .. } => Next::Ends(*outcome),
    }
}

/// The chips each player has put in at `node` of `game`: since the spot started, and on the
/// node's street. A chance node closes a street, so at one the street's chips are 0.
fn chips_at(game: &PostflopGame, node: u32) -> ([Chips; 2], [Chips; 2]) {
    let mut total = [0; 2];
    let mut street = [0; 2];
    for (at, index) in game.path(node) {
        match game.actor(at) {
            Some(actor) => {
                let (me, them) = (actor.index(), actor.opponent().index());
                match game.actions(at)[index] {
                    Action::Fold | Action::Check => {}
                    Action::Call => street[me] = street[them],
                    Action::Bet { to, .. } | Action::Raise { to, .. } | Action::AllIn { to } => {
                        street[me] = to;
                    }
                }
            }
            None => {
                for player in 0..2 {
                    total[player] += street[player];
                }
                street = [0; 2];
            }
        }
    }
    ([total[0] + street[0], total[1] + street[1]], street)
}

/// The last street a node needs loaded, from its path alone: the spot's first street, and one
/// more for each card on the path. A path's cards are its only tokens that read as cards.
pub fn street_of_path(first: Street, path: &str) -> Street {
    let cards = path
        .split_whitespace()
        .filter(|token| token.parse::<Card>().is_ok())
        .count();
    let mut street = first;
    for _ in 0..cards {
        street = street.next().unwrap_or(Street::River);
    }
    street
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_is_a_class_or_a_combo() {
        assert_eq!(
            HandQuery::parse("AKs"),
            Some(HandQuery::Class("AKs".parse().unwrap()))
        );
        assert_eq!(
            HandQuery::parse("t9O"),
            Some(HandQuery::Class("T9o".parse().unwrap()))
        );
        assert_eq!(
            HandQuery::parse("77"),
            Some(HandQuery::Class("77".parse().unwrap()))
        );
        assert_eq!(
            HandQuery::parse("ahKh"),
            Some(HandQuery::Combo("AhKh".parse().unwrap()))
        );
        for bad in ["AK", "AKx", "AhAh", "7", "", "AhKhQh"] {
            assert_eq!(HandQuery::parse(bad), None, "{bad}");
        }
        assert_eq!(HandQuery::parse("AKs").unwrap().combos().len(), 4);
        assert_eq!(HandQuery::parse("77").unwrap().combos().len(), 6);
        assert_eq!(HandQuery::parse("T9o").unwrap().combos().len(), 12);
    }

    #[test]
    fn a_path_needs_a_street_more_for_each_card() {
        assert_eq!(street_of_path(Street::Flop, ""), Street::Flop);
        assert_eq!(street_of_path(Street::Flop, "x b50 c"), Street::Flop);
        assert_eq!(street_of_path(Street::Flop, "x b50 c 8h"), Street::Turn);
        assert_eq!(
            street_of_path(Street::Flop, "x x 8H x x 3c b50"),
            Street::River
        );
        assert_eq!(street_of_path(Street::Turn, "x x 3c a"), Street::River);
        // A path with too many cards is refused when it is looked up.
        assert_eq!(street_of_path(Street::Turn, "3c 4c 5c"), Street::River);
    }
}
