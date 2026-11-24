//! The heads-up postflop betting tree.
//!
//! A [`PostflopTree`] holds every betting line from a root on the flop, turn or river to the
//! end of the hand. It does not depend on the cards: a chance node has one child, the betting
//! that follows whatever card falls, and a solver keeps its per-card data itself.
//!
//! # Inputs
//!
//! A [`TreeSpec`] gives the street the tree starts on, the pot at the root, the effective stack
//! at the root (the chips each player has left to bet) and a [`PostflopSizing`]. Amounts are
//! whole chips, with 1bb = 100 chips. The out-of-position player ([`Player::Oop`]) acts first on
//! every street and the in-position player ([`Player::Ip`]) second.
//!
//! The rules below talk about chips a player has *put in*: everything that player has bet or
//! called since the root, on any street. The *pot* at a decision is the root pot plus what both
//! players have put in.
//!
//! # Actions
//!
//! With no bet to face, a player checks or bets:
//!
//! - Each entry `f` of the street's `bets` gives a bet of `f` × the pot, rounded to whole chips
//!   by [`rules::round_chips`] (to the nearest chip, halves up). A bet below 1bb (100 chips) is
//!   lifted to 1bb, which makes it all-in, by the rule below, when less than 1bb remains.
//! - If the street's `allin` flag is set, going all-in is offered as one more bet.
//!
//! Facing a bet or raise, a player folds, calls or raises:
//!
//! - Each entry `m` of the street's `raises` gives a raise to `m` × the bet faced, rounded the
//!   same way. The bet faced is the opponent's total for the street. A raise below the minimum
//!   raise, which is the bet faced plus the last bet or raise increment on the street, is lifted
//!   to the minimum raise. After a bet of 100 the minimum raise is to 200, and after a raise from
//!   100 to 300 the next is to 500.
//! - If the street's `allin` flag is set, going all-in is offered as one more raise.
//! - A street allows `max_raises` raises after its opening bet, all-in raises included. Once it
//!   has had that many, a player facing a bet may only fold or call.
//! - A player who cannot put in more than the bet faced may only fold or call. A player who can,
//!   but not as much as a minimum raise, may raise all-in: every size then converts to all-in.
//!
//! Then, for bets and raises alike:
//!
//! - **All-in by stack.** A size that would use all of the player's remaining chips, or more,
//!   becomes all-in.
//! - **All-in by threshold.** A size that would leave the player with more than
//!   `allin_threshold` × the root effective stack put in also becomes all-in. Since chips are
//!   whole, this is the same as a total above [`rules::commit_limit`], the threshold times the
//!   stack rounded down. A threshold of 1 never converts anything.
//! - **Duplicates.** Sizes that end up at the same amount are one action. It is labelled with
//!   the smallest of those sizes, so the order of the sizes in the config does not matter.
//!
//! The actions at a node are in this order:
//!
//! - with no bet to face: check, then bets from smallest to largest;
//! - facing a bet: fold, call, then raises from smallest to largest.
//!
//! All-in is the largest amount a player can bet, so when it is offered it is always last.
//!
//! # How a street ends
//!
//! - A fold ends the hand. The other player wins.
//! - A call, or a check from IP after OOP has checked, ends the street. On the river that is a
//!   showdown. Before the river it leads to a chance node that deals the next street, and then to
//!   OOP's first decision on that street. If both players are all-in, it leads instead through a
//!   chance node for each street left to a showdown, with no decisions on the way.
//!
//! # Layout
//!
//! The tree is an arena: a `Vec` of [`Node`]s addressed by `u32` index, with the root at
//! [`PostflopTree::ROOT`]. Nodes are in depth-first pre-order: a node comes before its
//! children, the children follow the order of the node's actions, and every subtree takes a
//! contiguous range of indices.
//!
//! # Node paths
//!
//! A node is named by its *path*: the actions from the root to it, oldest first, each written as
//! its token (`Display` for [`Action`]) and separated by spaces, such as `x b50 c`. The empty
//! path is the root. [`PostflopTree::find`] turns a path into a node index. A token names the
//! action as the tree labels it, so a size that merged with a smaller one is reached by the
//! smaller size's token, and a size that became all-in by `a`.
//!
//! A path ends at the first chance node it reaches: the card it deals is not part of the path
//! yet. So on a flop or turn tree a path can name the decisions of the street it starts on and
//! the chance node that ends it, but nothing after.
//!
//! # Size
//!
//! Every bet and raise adds at least one chip, and no player can put in more than the stack, so
//! every line ends and the tree is finite. It can still be very large: the number of lines grows
//! with the sizes per street, the raise cap and the stack, and multiplies from street to street.
//! So [`TreeSpec::max_nodes`] caps it. A tree that would pass the cap is refused with
//! [`TreeError::TooLarge`], and the builder never holds more than that many nodes, made or
//! waiting to be made. [`TreeSpec::new`] sets the default of
//! [`TreeSpec::DEFAULT_MAX_NODES`], one million.
//!
//! # Inputs that make no sense
//!
//! [`PostflopTree::build`] rejects a stack of 0 (there would be no decision to make, and the
//! root would not be a player node), a pot and stacks whose total does not fit in [`Chips`], a
//! node limit of 0, and a sizing the config loader would reject. A pot of 0 is accepted: every
//! pot-fraction bet is then 0 chips and is lifted to 1bb.

use std::fmt;

use thiserror::Error;

use crate::config::{CHIPS_PER_BB, Chips, Invalid, PostflopSizing};
use crate::rules::{self, Stake};

/// A postflop street.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Street {
    Flop,
    Turn,
    River,
}

impl Street {
    pub const ALL: [Street; 3] = [Street::Flop, Street::Turn, Street::River];

    /// The street after this one, or `None` after the river.
    pub fn next(self) -> Option<Street> {
        match self {
            Street::Flop => Some(Street::Turn),
            Street::Turn => Some(Street::River),
            Street::River => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Street::Flop => "flop",
            Street::Turn => "turn",
            Street::River => "river",
        }
    }
}

impl fmt::Display for Street {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One of the two players in a heads-up postflop tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Player {
    /// Out of position: acts first on every street.
    Oop,
    /// In position: acts second on every street.
    Ip,
}

impl Player {
    /// 0 for OOP and 1 for IP: the index into per-player arrays such as contributions.
    pub const fn index(self) -> usize {
        match self {
            Player::Oop => 0,
            Player::Ip => 1,
        }
    }

    pub const fn opponent(self) -> Player {
        match self {
            Player::Oop => Player::Ip,
            Player::Ip => Player::Oop,
        }
    }
}

impl fmt::Display for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Player::Oop => "OOP",
            Player::Ip => "IP",
        })
    }
}

/// An action at a player node. Amounts are the actor's total for the street after the action.
///
/// `Display` prints the token that names the action in a node path: `f`, `x`, `c`, `b33` (a bet
/// from the size 0.33), `r3` (a raise from the size 3) or `a`. A bet or raise keeps the label of
/// the size it came from even when the minimum bet or the minimum raise lifted it; its amount is
/// in `to`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Fold,
    Check,
    Call,
    /// An opening bet, from the size `fraction` of the pot.
    Bet {
        to: Chips,
        fraction: f64,
    },
    /// A raise, from the size `multiple` of the bet faced.
    Raise {
        to: Chips,
        multiple: f64,
    },
    /// A bet or raise of every chip the actor has left.
    AllIn {
        to: Chips,
    },
}

impl Action {
    /// The actor's street total after a bet, raise or all-in. `None` for the other actions.
    pub fn to(self) -> Option<Chips> {
        match self {
            Action::Bet { to, .. } | Action::Raise { to, .. } | Action::AllIn { to } => Some(to),
            Action::Fold | Action::Check | Action::Call => None,
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Six decimal places are plenty for a size, and keep 0.33 x 100 from printing as
        // 33.00000000000001.
        let short = |x: f64| (x * 1e6).round() / 1e6;
        match *self {
            Action::Fold => f.write_str("f"),
            Action::Check => f.write_str("x"),
            Action::Call => f.write_str("c"),
            Action::Bet { fraction, .. } => write!(f, "b{}", short(fraction * 100.0)),
            Action::Raise { multiple, .. } => write!(f, "r{}", short(multiple)),
            Action::AllIn { .. } => f.write_str("a"),
        }
    }
}

/// How a terminal node ends the hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// One player folded and `winner` takes the pot.
    Fold { winner: Player },
    /// The hands are shown down on the river.
    Showdown,
}

/// A node of the tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A decision by `actor`. `children[i]` is the node that `actions[i]` leads to.
    Player {
        street: Street,
        actor: Player,
        actions: Vec<Action>,
        children: Vec<u32>,
    },
    /// Deals the card for `street`, the turn or the river. Its one child is the betting that
    /// follows whatever card falls.
    Chance { street: Street, child: u32 },
    /// The end of the hand. `contributions` holds the chips each player put in during this tree,
    /// indexed by [`Player::index`]; the pot is the root pot plus both. A showdown is always on
    /// the river, with equal contributions.
    Terminal {
        street: Street,
        outcome: Outcome,
        contributions: [Chips; 2],
    },
}

impl Node {
    /// The street the node belongs to. A chance node belongs to the street it deals.
    pub fn street(&self) -> Street {
        match *self {
            Node::Player { street, .. }
            | Node::Chance { street, .. }
            | Node::Terminal { street, .. } => street,
        }
    }

    /// The node's children: one per action at a player node, one at a chance node and none at a
    /// terminal.
    pub fn children(&self) -> &[u32] {
        match self {
            Node::Player { children, .. } => children,
            Node::Chance { child, .. } => std::slice::from_ref(child),
            Node::Terminal { .. } => &[],
        }
    }
}

/// What a tree is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeSpec {
    /// The street the root is on.
    pub street: Street,
    /// The pot at the root.
    pub pot: Chips,
    /// The effective stack at the root: what each player has left to bet.
    pub stack: Chips,
    pub sizing: PostflopSizing,
    /// The most nodes the tree may have. A larger tree is refused with [`TreeError::TooLarge`]
    /// before it takes more memory than this many nodes need. [`TreeSpec::new`] sets
    /// [`TreeSpec::DEFAULT_MAX_NODES`].
    pub max_nodes: u32,
}

impl TreeSpec {
    /// One million nodes. A node takes roughly 100 bytes with its actions, so the builder stays
    /// near 100 MB at most. A tree this large is already far past what an exact-card solve can
    /// hold: each player node needs regrets and strategy sums for every in-range combo.
    pub const DEFAULT_MAX_NODES: u32 = 1_000_000;

    /// A spec with the default node limit.
    pub fn new(street: Street, pot: Chips, stack: Chips, sizing: PostflopSizing) -> TreeSpec {
        TreeSpec {
            street,
            pot,
            stack,
            sizing,
            max_nodes: TreeSpec::DEFAULT_MAX_NODES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TreeError {
    #[error("`{field}` {reason}")]
    Invalid { field: String, reason: String },
    #[error("the tree has more than {limit} nodes, the limit set by `max_nodes`")]
    TooLarge { limit: u32 },
}

/// Why a node path does not lead to a node: see [`PostflopTree::find`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathError {
    /// The tokens before the one that failed, which lead to a node.
    pub before: Vec<String>,
    /// The token that failed.
    pub token: String,
    pub kind: PathErrorKind,
}

/// What is at the node where a path fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathErrorKind {
    /// A player node with no action of that token. `actions` are its tokens, in order.
    NoAction { actions: Vec<String> },
    /// A terminal: the hand is over, and no token can follow.
    HandOver { outcome: Outcome },
    /// A chance node, which deals `street`. A path does not go past one yet.
    Chance { street: Street },
}

/// One line, such as "`b75` is not an action after `x`; the actions there are `x` and `b50`".
impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (token, before) = (&self.token, self.before.join(" "));
        match &self.kind {
            PathErrorKind::NoAction { actions } => {
                let place = if before.is_empty() {
                    "at the root".to_string()
                } else {
                    format!("after `{before}`")
                };
                let actions: Vec<String> = actions.iter().map(|a| format!("`{a}`")).collect();
                let actions = match actions.split_last() {
                    Some((last, rest)) if !rest.is_empty() => {
                        format!("{} and {last}", rest.join(", "))
                    }
                    _ => actions.concat(),
                };
                write!(
                    f,
                    "`{token}` is not an action {place}; the actions there are {actions}"
                )
            }
            PathErrorKind::HandOver { outcome } => {
                let end = match outcome {
                    Outcome::Fold { winner } => format!("where {} folds", winner.opponent()),
                    Outcome::Showdown => "a showdown".to_string(),
                };
                write!(
                    f,
                    "`{token}` comes after the end of the hand, at `{before}`, {end}"
                )
            }
            PathErrorKind::Chance { street } => write!(
                f,
                "`{token}` comes after `{before}`, which deals the {street}, and a path does not \
                 go past a chance node yet"
            ),
        }
    }
}

impl std::error::Error for PathError {}

impl From<Invalid> for TreeError {
    fn from(invalid: Invalid) -> TreeError {
        TreeError::Invalid {
            field: invalid.field,
            reason: invalid.reason,
        }
    }
}

/// Node counts for one street.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreetCounts {
    /// Player nodes.
    pub player: usize,
    /// Actions summed over the player nodes.
    pub actions: usize,
    /// Chance nodes that deal this street.
    pub chance: usize,
    /// Terminals where a player folded on this street.
    pub fold: usize,
    /// Showdowns. Only the river has any.
    pub showdown: usize,
}

impl StreetCounts {
    /// Every node on the street.
    pub fn nodes(&self) -> usize {
        self.player + self.chance + self.fold + self.showdown
    }
}

/// Node counts per street. Each node counts on the street it belongs to (see [`Node::street`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeCounts {
    pub flop: StreetCounts,
    pub turn: StreetCounts,
    pub river: StreetCounts,
}

impl NodeCounts {
    pub fn street(&self, street: Street) -> &StreetCounts {
        match street {
            Street::Flop => &self.flop,
            Street::Turn => &self.turn,
            Street::River => &self.river,
        }
    }

    fn street_mut(&mut self, street: Street) -> &mut StreetCounts {
        match street {
            Street::Flop => &mut self.flop,
            Street::Turn => &mut self.turn,
            Street::River => &mut self.river,
        }
    }

    /// Every node in the tree.
    pub fn nodes(&self) -> usize {
        Street::ALL.iter().map(|&s| self.street(s).nodes()).sum()
    }
}

/// A heads-up postflop betting tree. The module documentation states its rules.
#[derive(Debug, Clone, PartialEq)]
pub struct PostflopTree {
    spec: TreeSpec,
    nodes: Vec<Node>,
}

impl PostflopTree {
    /// The index of the root.
    pub const ROOT: u32 = 0;

    /// Builds the tree. Fails if an input makes no sense, or if the tree would have more than
    /// `spec.max_nodes` nodes.
    pub fn build(spec: TreeSpec) -> Result<PostflopTree, TreeError> {
        spec.sizing.check("sizing")?;
        if spec.stack == 0 {
            let reason = "is 0; the players need chips to bet for there to be a decision";
            return Err(Invalid::new("stack", reason).into());
        }
        if spec.max_nodes == 0 {
            return Err(Invalid::new("max_nodes", "is 0; a tree has at least its root").into());
        }
        if u64::from(spec.pot) + 2 * u64::from(spec.stack) > u64::from(Chips::MAX) {
            let reason = format!(
                "is {} with a stack of {}; the pot plus both stacks must fit in {} chips",
                spec.pot,
                spec.stack,
                Chips::MAX
            );
            return Err(Invalid::new("pot", reason).into());
        }
        let nodes = Builder::new(&spec).build()?;
        Ok(PostflopTree { spec, nodes })
    }

    pub fn spec(&self) -> &TreeSpec {
        &self.spec
    }

    /// Every node, in pre-order from the root.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node(&self, index: u32) -> &Node {
        &self.nodes[index as usize]
    }

    /// The node that `path` leads to from the root, as the
    /// [module documentation](self#node-paths) describes paths: tokens separated by any
    /// whitespace, such as `x b50 c`, with `""` for the root.
    ///
    /// Fails at the first token that does not lead on: one that is not an action at the node
    /// the tokens before it reach, which the error lists the actions of, or one that comes after
    /// the end of the hand or after a chance node.
    pub fn find(&self, path: &str) -> Result<u32, PathError> {
        let mut at = PostflopTree::ROOT;
        let mut before: Vec<String> = Vec::new();
        for token in path.split_whitespace() {
            let found = match self.node(at) {
                Node::Player {
                    actions, children, ..
                } => match actions.iter().position(|a| a.to_string() == token) {
                    Some(slot) => Ok(children[slot]),
                    None => Err(PathErrorKind::NoAction {
                        actions: actions.iter().map(Action::to_string).collect(),
                    }),
                },
                Node::Chance { street, .. } => Err(PathErrorKind::Chance { street: *street }),
                Node::Terminal { outcome, .. } => {
                    Err(PathErrorKind::HandOver { outcome: *outcome })
                }
            };
            match found {
                Ok(child) => at = child,
                Err(kind) => {
                    return Err(PathError {
                        before,
                        token: token.to_string(),
                        kind,
                    });
                }
            }
            before.push(token.to_string());
        }
        Ok(at)
    }

    /// Node counts per street, for the memory estimate.
    pub fn counts(&self) -> NodeCounts {
        let mut counts = NodeCounts::default();
        for node in &self.nodes {
            let street = counts.street_mut(node.street());
            match node {
                Node::Player { actions, .. } => {
                    street.player += 1;
                    street.actions += actions.len();
                }
                Node::Chance { .. } => street.chance += 1,
                Node::Terminal {
                    outcome: Outcome::Fold { .. },
                    ..
                } => street.fold += 1,
                Node::Terminal {
                    outcome: Outcome::Showdown,
                    ..
                } => street.showdown += 1,
            }
        }
        counts
    }
}

/// The betting so far, at a decision.
#[derive(Debug, Clone, Copy)]
struct State {
    street: Street,
    actor: Player,
    /// Chips each player has put in since the root.
    total: [Chips; 2],
    /// Chips each player has put in on this street.
    street_bet: [Chips; 2],
    /// The last bet or raise increment on this street; 0 before anyone bets.
    last_increment: Chips,
    /// Raises on this street after the opening bet.
    raises: u32,
}

impl State {
    fn street_start(street: Street, total: [Chips; 2]) -> State {
        State {
            street,
            actor: Player::Oop,
            total,
            street_bet: [0, 0],
            last_increment: 0,
            raises: 0,
        }
    }
}

/// A node the builder has still to make.
enum Pending {
    Decision(State),
    /// Deals `street`, with both players having put in `total`.
    Chance {
        street: Street,
        total: [Chips; 2],
    },
    Terminal {
        street: Street,
        outcome: Outcome,
        contributions: [Chips; 2],
    },
}

struct Builder<'a> {
    spec: &'a TreeSpec,
    /// The most a player may have put in after a bet or raise that is not all-in.
    limit: Chips,
}

impl<'a> Builder<'a> {
    fn new(spec: &'a TreeSpec) -> Builder<'a> {
        Builder {
            spec,
            limit: rules::commit_limit(spec.stack, spec.sizing.allin_threshold),
        }
    }

    /// Builds the nodes in pre-order. An explicit stack stands in for recursion, because a deep
    /// stack and small raises can make lines far longer than the call stack allows.
    ///
    /// Every node waiting on the stack will be made, so once the nodes made plus the nodes
    /// waiting pass `max_nodes`, the tree is too large and the build stops there. Memory is
    /// bounded by the limit, whatever the sizing.
    fn build(&self) -> Result<Vec<Node>, TreeError> {
        let limit = self.spec.max_nodes;
        let root = Pending::Decision(State::street_start(self.spec.street, [0, 0]));
        let mut nodes: Vec<Node> = Vec::new();
        // Each entry is a node to make, and the parent and child slot that should point at it.
        let mut work: Vec<(Pending, Option<(u32, usize)>)> = vec![(root, None)];
        while let Some((pending, parent)) = work.pop() {
            // Below `limit`, because made plus waiting never passes it.
            let index = u32::try_from(nodes.len()).expect("the node limit is a u32");
            if let Some((parent, slot)) = parent {
                match &mut nodes[parent as usize] {
                    Node::Player { children, .. } => children[slot] = index,
                    Node::Chance { child, .. } => *child = index,
                    Node::Terminal { .. } => unreachable!("a terminal has no children"),
                }
            }
            match pending {
                Pending::Decision(state) => {
                    let actions = self.actions(&state);
                    // Pushed in reverse, so that the first action's subtree is made first.
                    for (slot, &action) in actions.iter().enumerate().rev() {
                        work.push((self.after(&state, action), Some((index, slot))));
                    }
                    nodes.push(Node::Player {
                        street: state.street,
                        actor: state.actor,
                        children: vec![0; actions.len()],
                        actions,
                    });
                }
                Pending::Chance { street, total } => {
                    work.push((self.dealt(street, total), Some((index, 0))));
                    nodes.push(Node::Chance { street, child: 0 });
                }
                Pending::Terminal {
                    street,
                    outcome,
                    contributions,
                } => nodes.push(Node::Terminal {
                    street,
                    outcome,
                    contributions,
                }),
            }
            if nodes.len() + work.len() > limit as usize {
                return Err(TreeError::TooLarge { limit });
            }
        }
        Ok(nodes)
    }

    /// The actions at a decision, in the documented order.
    fn actions(&self, state: &State) -> Vec<Action> {
        let me = state.actor.index();
        let opponent = state.actor.opponent().index();
        let sizing = self.spec.sizing.street(state.street);
        let actor = Stake {
            street: state.street_bet[me],
            total: state.total[me],
            behind: self.spec.stack - state.total[me],
        };
        let faced = state.street_bet[opponent];

        if faced == actor.street {
            // No bet to face. Both street totals are 0 here: the street has just started, or OOP
            // has checked.
            let pot = self.spec.pot + state.total[0] + state.total[1];
            let bets = rules::bets(
                pot,
                &sizing.bets,
                sizing.allin,
                CHIPS_PER_BB,
                actor,
                self.limit,
            );
            let bets = bets.into_iter().map(|wager| match wager.size {
                Some(fraction) => Action::Bet {
                    to: wager.to,
                    fraction,
                },
                None => Action::AllIn { to: wager.to },
            });
            std::iter::once(Action::Check).chain(bets).collect()
        } else {
            let raises = if rules::under_raise_cap(state.raises, self.spec.sizing.max_raises) {
                rules::raises(
                    faced,
                    state.last_increment,
                    &sizing.raises,
                    sizing.allin,
                    actor,
                    self.limit,
                )
            } else {
                Vec::new()
            };
            let raises = raises.into_iter().map(|wager| match wager.size {
                Some(multiple) => Action::Raise {
                    to: wager.to,
                    multiple,
                },
                None => Action::AllIn { to: wager.to },
            });
            [Action::Fold, Action::Call]
                .into_iter()
                .chain(raises)
                .collect()
        }
    }

    /// The node that `action` at `state` leads to.
    fn after(&self, state: &State, action: Action) -> Pending {
        let me = state.actor.index();
        let opponent = state.actor.opponent();
        match action {
            Action::Fold => Pending::Terminal {
                street: state.street,
                outcome: Outcome::Fold { winner: opponent },
                contributions: state.total,
            },
            Action::Check if state.actor == Player::Oop => Pending::Decision(State {
                actor: Player::Ip,
                ..*state
            }),
            Action::Check => self.street_end(state.street, state.total),
            Action::Call => {
                let mut total = state.total;
                total[me] += state.street_bet[opponent.index()] - state.street_bet[me];
                self.street_end(state.street, total)
            }
            Action::Bet { to, .. } | Action::Raise { to, .. } | Action::AllIn { to } => {
                let faced = state.street_bet[opponent.index()];
                let facing = faced > state.street_bet[me];
                let mut next = *state;
                next.total[me] += to - state.street_bet[me];
                next.street_bet[me] = to;
                next.last_increment = rules::next_increment(faced, to, state.last_increment);
                if facing {
                    next.raises += 1;
                }
                next.actor = opponent;
                Pending::Decision(next)
            }
        }
    }

    /// What follows the end of the betting on `street`, with both players having put in `total`.
    fn street_end(&self, street: Street, total: [Chips; 2]) -> Pending {
        debug_assert_eq!(total[0], total[1], "a street ends with equal totals");
        match street.next() {
            Some(next) => Pending::Chance {
                street: next,
                total,
            },
            None => Pending::Terminal {
                street,
                outcome: Outcome::Showdown,
                contributions: total,
            },
        }
    }

    /// What follows the chance node that deals `street`.
    fn dealt(&self, street: Street, total: [Chips; 2]) -> Pending {
        if total[0] == self.spec.stack {
            // Both players are all-in: no betting on this street.
            self.street_end(street, total)
        } else {
            Pending::Decision(State::street_start(street, total))
        }
    }
}

#[cfg(test)]
mod tests;
