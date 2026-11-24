//! The game of a spot: a [`Spot`] on any street as a [`PublicGame`], which [`VectorCfr`] solves.
//!
//! # Players and hands
//!
//! Player 0 is OOP and player 1 is IP, as [`Player::index`] numbers them. A player's hands are
//! its [`HandList`] on the spot's board ([`crate::hands`]): the combos of its range that have a
//! positive weight and miss the board, in combo order. A combo the board blocks, or one outside
//! the range, has no hand index, so no per-hand vector of the solver holds it, and each player
//! node stores regrets and strategy sums for the acting player's in-range combos only.
//!
//! The lists are those of the spot's board for the whole game, whatever falls later
//! (DECISIONS P25). A turn or river card *rules out* the hands that hold it: they cannot be held
//! below the chance node that dealt it, keep their place in every per-hand vector there, and
//! have reach zero.
//!
//! # The deal
//!
//! Chance deals OOP the hand `h` and IP the hand `o` with probability proportional to the
//! product of their range weights, `w_0(h) · w_1(o)`, if the two combos share no card, and never
//! otherwise. Normalised so that the probabilities sum to 1 over the pairs that can be dealt:
//!
//! ```text
//! Z          = Σ w_0(h) · w_1(o)    over the pairs (h, o) that share no card
//! deal(p)[h] = w_p(h) / √Z
//! ```
//!
//! so that `deal(0)[h] · deal(1)[o] = w_0(h) · w_1(o) / Z`, and every value computed from the
//! deal, such as a player's expected value or best response, is in chips per hand dealt.
//! [`PostflopGame::pair_weight`] is `Z`, which [`BoardHands::fold_values`] gives in O(n). Any
//! split of `1 / Z` between the players gives the same strategies; splitting it evenly keeps the
//! two players' stored regrets on the same scale. The ranges are the ranges on the spot's board,
//! and the cards that fall later do not change the deal: they rule hands out.
//!
//! # The public tree
//!
//! The betting tree ([`PostflopTree`]) does not depend on the cards: its chance node has one
//! child, the betting after whatever card falls. The public tree expands it (DECISIONS P23):
//! each chance node has one child per card that can fall, each the root of a copy of the
//! betting below the chance node. Every public node knows its betting-tree node
//! ([`PostflopGame::tree_node`]) and the board as it stands there ([`PostflopGame::board`]).
//! The nodes are in depth-first pre-order from the root at 0, children in the order of the
//! node's actions or outcomes, so every subtree takes a contiguous range of indices.
//!
//! - A player node's actions are its betting node's, in the tree's order, and the acting player
//!   is the actor's [`Player::index`].
//! - A chance node's outcomes are the cards not on its board, in card order ([`Card::index`]):
//!   49 turn cards after a flop, 48 river cards after a turn. Outcome `c` rules out each
//!   player's hands that hold `c`. Once both players hold their hands, the next card is equally
//!   likely to be any of the `48 − |board|` cards neither holds, so every outcome has
//!   probability `1 / (48 − |board|)`, 1/45 for the turn and 1/44 for the river, and for any two
//!   hands that can be held together at the node, the outcomes that rule out neither sum to 1
//!   (DECISIONS P24).
//! - A terminal is a betting terminal on the board it is reached on.
//!
//! A spot that starts on the river has no chance node, and its public tree is the betting tree
//! node for node: public node `n` is tree node `n`. The information sets are the pairs of a
//! player node and one of the acting player's hands that is possible there, holding no card
//! dealt on the way.
//!
//! # Terminals
//!
//! Each betting terminal's [`Payoff`] is computed once, when the game is built, from the root
//! pot, the terminal's outcome and contributions, and the spot's rake ([`crate::payoff`]).
//! Payoffs are net chips from the start of the spot, so over both players they sum to the pot
//! less the rake: the game is not zero-sum, which is why
//! [exploitability](mod@crate::exploitability) has a definition of its own.
//!
//! A public terminal is evaluated on its own board (DECISIONS P26), with
//! [`BoardHands::fold_values`] after a fold, on any street, and [`BoardHands::showdown_values`]
//! at a showdown, which is always on a river board, each in O(n) with card removal. The game
//! builds a [`BoardHands`] for every board the public tree reaches: the spot's board, each turn
//! and each river. A river board's hands are ranked once, when it is built, and serve every
//! showdown on it, and the river reached by the turn card `a` and the river card `b` is the same
//! board as the one reached by `b` and then `a`. A board's own hand lists hold only the hands
//! that miss it, so the evaluation gathers the opponent's reach into the board's order and
//! writes the values back by an index into the spot's hand lists. A hand that holds a card
//! dealt on the way gets 0, and so does nothing that the opponent's ruled-out hands could add.
//! On the spot's own board the evaluation is direct, which is all a river spot does.
//!
//! # Size
//!
//! The public tree grows with the cards: a flop line that reaches the river is copied 49 × 48
//! times. [`PostflopGame::with_node_limit`] counts its nodes from the betting tree before it
//! builds anything, and refuses a tree over the limit with [`SolveError::PublicTreeTooLarge`]
//! (DECISIONS P28). The default limit, [`PostflopGame::DEFAULT_MAX_NODES`], is a million nodes,
//! the betting tree's own default. It bounds the tree, not the solver's storage, which grows
//! with the hands as well and is the memory estimate's to bound.
//!
//! # Node paths
//!
//! A public node is named by its *path*: the actions and the cards from the root to it, oldest
//! first, each written as a token and separated by spaces. An action's token is the one
//! [`pk_game::tree`] prints (`x`, `c`, `f`, `b50`, `r3` or `a`), and the card a chance node deals
//! is written as a card, such as `7s`, in either case. So `x b50 c 7s x` is the node after OOP
//! checks, IP bets half the pot and OOP calls on the flop, the seven of spades falls, and OOP
//! checks on the turn. The empty path is the root. [`PostflopGame::find`] turns a path into a
//! node, and [`PostflopGame::node_path`] gives a node's path (DECISIONS P29).
//!
//! # Ranges at a node
//!
//! For reading a strategy, [`PostflopGame::reach`] gives each player's own probability of
//! playing to a node with each hand: the product of its own action probabilities on the way,
//! and 0 for a hand that holds a card dealt on the way. Summing a strategy over the acting
//! player's range at a node can then answer two different questions, and the two answers
//! differ.
//!
//! **The probability of each action once the node is reached.** [`PostflopGame::hand_weights`]
//! gives the probability that the acting player holds each hand once the node is reached. For
//! the acting player `i`, the opponent `j` and own reaches `π` under a profile,
//!
//! ```text
//! P(h | n) ∝ deal(i)[h] · π_i(n, h) · Σ_o deal(j)[o] · π_j(n, o)    over o sharing no card with h
//! ```
//!
//! which is exact, card removal by the opponent's range at the node included. The cards dealt
//! on the way have the same probability for every pair of hands that can reach the node, so
//! they do not enter. [`PostflopGame::action_frequencies`] is then `Σ_h P(h | n) · σ(a | h)`:
//! how often the action is actually taken when play reaches the node. Use it for the
//! probabilities of the play, such as how often a bet is called, or for anything weighted by
//! how often a line happens.
//!
//! **What a range view shows.** [`PostflopGame::range_view_weights`] weighs each hand by the
//! player's own combos alone, its range weight times its own reach:
//!
//! ```text
//! W(h | n) = w_i(h) · π_i(n, h) / Σ_h' w_i(h') · π_i(n, h')
//! ```
//!
//! with no card removal by the opponent's range, and [`PostflopGame::range_view_frequencies`]
//! is `Σ_h W(h | n) · σ(a | h)`. This is what a solver's range view shows, and what a person
//! compares between two solvers, so the cross-solver comparison
//! ([`compare`](mod@crate::compare)) uses it. A hand that holds a card dealt on the way has a
//! reach of 0, so it is not in the range below that card.
//!
//! The two differ wherever the opponent's range at the node blocks some of the player's hands
//! more than others. On `four_flush`, solved to 0.05% of the pot, IP's re-raise facing OOP's
//! check-raise, a node reached about 4% of the time, is 10.1% by the first and 18.7% by the
//! second: the hands IP re-raises with share cards with OOP's check-raising range more often
//! than its other hands do. A test pins the two apart.
//!
//! **A range view by hand class.** [`PostflopGame::range_view_classes`] sums the range view over
//! each of the 169 hand classes ([`HandClass`]) in the acting player's range at the node: the
//! class's share of the range, `S(c | n) = Σ W(h | n)` over its hands, and each action's
//! frequency over its hands under the same weights, `Σ W(h | n) · σ(a | h) / S(c | n)`. The
//! shares sum to 1, and the shares times the class frequencies sum to the range view's
//! frequencies.
//!
//! **How often a node is reached.** [`PostflopGame::reach_probability`] is the probability that
//! a hand dealt plays to the node: `Σ deal(0)[h] · π_0(n, h) · deal(1)[o] · π_1(n, o)` over the
//! pairs of hands that share no card, times the probability of each card dealt on the way. Over
//! the children of a chance node it sums to the chance node's.
//!
//! [`VectorCfr`]: pk_cfr::VectorCfr

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use pk_cfr::{PublicGame, PublicNode, PublicProfile};
use pk_core::{Card, CardSet, HandClass, NUM_CARDS};
use pk_game::{Action, Chips, Node, Outcome, Player, PostflopTree, Rake, Street};

use crate::hands::HandList;
use crate::payoff::Payoff;
use crate::solve::SolveError;
use crate::spot::Spot;
use crate::terminal::BoardHands;

/// One hand class in the acting player's range at a node, as a range view shows it: see
/// [`PostflopGame::range_view_classes`].
#[derive(Debug, Clone, PartialEq)]
pub struct ClassView {
    pub class: HandClass,
    /// The acting player's hands in the class, as hand indices in hand order, whether or not
    /// each plays to the node, or can be held there.
    pub hands: Vec<usize>,
    /// The class's share of the range at the node: the sum of its hands'
    /// [`range_view_weights`](PostflopGame::range_view_weights). Always more than 0.
    pub share: f64,
    /// Each action's frequency over the class's hands, each hand weighted by its range-view
    /// weight, in the order of the node's actions.
    pub frequencies: Vec<f64>,
}

/// A spot on any street as a public game, as the [module documentation](self) describes it.
#[derive(Debug, Clone)]
pub struct PostflopGame {
    tree: PostflopTree,
    /// The public tree, in pre-order from the root.
    nodes: Vec<PublicTreeNode>,
    /// Every board the public tree reaches, the spot's first.
    boards: Vec<Board>,
    pot: Chips,
    rake: Rake,
    /// `Z`, the total weight of the pairs of hands that can be dealt.
    pair_weight: f64,
    /// [`PublicGame::deal`] for OOP, then IP.
    deal: [Vec<f64>; 2],
    /// By betting-tree node: the payoffs at a terminal, and `None` elsewhere.
    payoffs: Vec<Option<Payoff>>,
}

/// One node of the public tree.
#[derive(Debug, Clone)]
struct PublicTreeNode {
    /// The betting-tree node.
    tree: u32,
    /// The board here, as an index into [`PostflopGame::boards`].
    board: u32,
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    /// `children[a]` follows action `a` of the betting node.
    Player {
        actor: Player,
        children: Vec<u32>,
    },
    /// `children[o]` follows outcome `o`, which deals `cards[o]`, with `probability`.
    Chance {
        cards: Vec<Card>,
        children: Vec<u32>,
        probability: f64,
    },
    Terminal,
}

/// One board the public tree reaches, with what its terminals are evaluated with.
#[derive(Debug, Clone)]
struct Board {
    hands: BoardHands,
    /// For each player, the hand index in the spot's list of each hand in this board's list.
    /// Empty for the spot's own board, whose lists are the spot's.
    to_root: [Vec<u32>; 2],
}

/// The player of index 0 or 1.
pub(crate) fn player_of(index: usize) -> Player {
    match index {
        0 => Player::Oop,
        1 => Player::Ip,
        _ => panic!("player {index} is not 0 or 1"),
    }
}

/// The number of cards on the board before a chance node deals `street`.
fn cards_before(street: Street) -> u64 {
    match street {
        Street::Turn => 3,
        Street::River => 4,
        Street::Flop => unreachable!("no chance node deals the flop"),
    }
}

/// The number of nodes in the public tree of `tree`, counted from the betting tree alone, or
/// `u64::MAX` if there are more.
fn public_size(tree: &PostflopTree) -> u64 {
    let nodes = tree.nodes();
    // Children come after their parent in pre-order, so a reverse pass sees them first.
    let mut size = vec![0u64; nodes.len()];
    for index in (0..nodes.len()).rev() {
        size[index] = match &nodes[index] {
            Node::Player { children, .. } => children.iter().fold(1u64, |total, &child| {
                total.saturating_add(size[child as usize])
            }),
            Node::Chance { street, child } => {
                let outcomes = NUM_CARDS as u64 - cards_before(*street);
                outcomes
                    .saturating_mul(size[*child as usize])
                    .saturating_add(1)
            }
            Node::Terminal { .. } => 1,
        };
    }
    size[0]
}

impl Board {
    /// The hands on `cards`, a board that extends the spot's, indexed against `root`, the
    /// spot's.
    fn new(cards: CardSet, spot: &Spot, root: &BoardHands) -> Board {
        let hands = BoardHands::new(cards, &spot.oop_range, &spot.ip_range);
        let to_root = [Player::Oop, Player::Ip].map(|player| {
            let list = root.hands(player);
            hands
                .hands(player)
                .combos()
                .iter()
                .map(|&combo| {
                    let hand = list
                        .position(combo)
                        .expect("a hand that misses a board misses the board it extends");
                    hand as u32
                })
                .collect()
        });
        Board { hands, to_root }
    }
}

impl PostflopGame {
    /// The largest public tree [`PostflopGame::from_spot`] builds: one million nodes, the
    /// betting tree's default limit.
    pub const DEFAULT_MAX_NODES: u64 = 1_000_000;

    /// The game of `spot`, which must start on the river: the constructor of the river game,
    /// which the name [`RiverGame`](crate::RiverGame) keeps (DECISIONS P27). Code that needs a
    /// river game, such as a [solution dump](crate::dump), gets an error from it rather than a
    /// flop game. [`PostflopGame::from_spot`] takes a spot on any street.
    ///
    /// Fails with [`SolveError::NotRiver`] if the board does not have five cards, and otherwise
    /// as [`PostflopGame::from_spot`] does.
    pub fn new(spot: &Spot) -> Result<PostflopGame, SolveError> {
        if spot.board.len() != 5 {
            return Err(SolveError::NotRiver {
                cards: spot.board.len(),
            });
        }
        PostflopGame::from_spot(spot)
    }

    /// The game of `spot`, on the flop, the turn or the river, with the default node limit.
    ///
    /// Fails as [`PostflopGame::with_node_limit`] does.
    pub fn from_spot(spot: &Spot) -> Result<PostflopGame, SolveError> {
        PostflopGame::with_node_limit(spot, PostflopGame::DEFAULT_MAX_NODES)
    }

    /// The game of `spot`, on the flop, the turn or the river, refusing a public tree of more
    /// than `max_nodes` nodes before it is built. Nodes are numbered by `u32`, so a limit above
    /// `u32::MAX` counts as that.
    ///
    /// Fails with [`SolveError::Spot`] if the spot does not pass [`Spot::validate`], with
    /// [`SolveError::Tree`] if its betting tree has more than `max_nodes` nodes, or if
    /// `max_nodes` is 0, and with [`SolveError::PublicTreeTooLarge`] if the public tree would
    /// have more than `max_nodes`.
    pub fn with_node_limit(spot: &Spot, max_nodes: u64) -> Result<PostflopGame, SolveError> {
        spot.validate()?;
        let limit = max_nodes.min(u64::from(u32::MAX));
        let mut spec = spot.tree_spec();
        spec.max_nodes = u32::try_from(limit).expect("the limit is at most u32::MAX");
        let tree = PostflopTree::build(spec)?;
        let size = public_size(&tree);
        if size > limit {
            return Err(SolveError::PublicTreeTooLarge { nodes: size, limit });
        }
        let hands = spot.board_hands();
        let rake = spot.rake();
        let payoffs = tree
            .nodes()
            .iter()
            .map(|node| match *node {
                Node::Terminal {
                    outcome,
                    contributions,
                    ..
                } => Some(Payoff::new(spot.pot, outcome, contributions, &rake)),
                Node::Player { .. } | Node::Chance { .. } => None,
            })
            .collect();
        let weights =
            [Player::Oop, Player::Ip].map(|player| hands.hands(player).weights().to_vec());
        // For each OOP hand, the weight of the IP hands it can be dealt with.
        let mut met = vec![0.0; weights[0].len()];
        hands.fold_values(Player::Oop, &weights[1], 1.0, &mut met);
        let pair_weight: f64 = weights[0].iter().zip(&met).map(|(w, m)| w * m).sum();
        // `Spot::validate` checks that some pair of hands can be dealt, and every weight in a
        // hand list is positive.
        assert!(
            pair_weight > 0.0,
            "a valid spot has a pair of hands that can be dealt together"
        );
        let scale = pair_weight.sqrt();
        let deal = weights.map(|weights| weights.iter().map(|w| w / scale).collect());
        let (nodes, boards) = expand(&tree, spot, hands, size as usize);
        Ok(PostflopGame {
            tree,
            nodes,
            boards,
            pot: spot.pot,
            rake,
            pair_weight,
            deal,
            payoffs,
        })
    }

    /// The betting tree. Public node `n` is the betting node [`PostflopGame::tree_node`] gives,
    /// which on a river spot is `n` itself.
    pub fn tree(&self) -> &PostflopTree {
        &self.tree
    }

    /// Both players' hands on the spot's board, with the terminal evaluations there.
    pub fn board_hands(&self) -> &BoardHands {
        &self.boards[0].hands
    }

    /// One player's hands. Hand `h` of player `player.index()` is `hands(player).combos()[h]`.
    pub fn hands(&self, player: Player) -> &HandList {
        self.boards[0].hands.hands(player)
    }

    /// The pot at the root, in chips.
    pub fn pot(&self) -> Chips {
        self.pot
    }

    /// The spot's rake, from which the terminal payoffs were computed.
    pub fn rake(&self) -> &Rake {
        &self.rake
    }

    /// `Z`, the sum of `w_0(h) · w_1(o)` over the pairs of hands that share no card.
    pub fn pair_weight(&self) -> f64 {
        self.pair_weight
    }

    /// The payoffs at a public terminal, which are its betting terminal's, or `None` at
    /// another node. Panics if `node` is out of range.
    pub fn payoff(&self, node: u32) -> Option<Payoff> {
        self.payoffs[self.public(node).tree as usize]
    }

    /// The betting-tree node of the public node `node`. Panics if `node` is out of range.
    pub fn tree_node(&self, node: u32) -> u32 {
        self.public(node).tree
    }

    /// The board at the public node `node`: the spot's board and every card dealt on the way.
    /// Panics if `node` is out of range.
    pub fn board(&self, node: u32) -> CardSet {
        self.boards[self.public(node).board as usize].hands.board()
    }

    /// The street of the public node `node`, as [`Node::street`] gives it for its betting node:
    /// a chance node is on the street it deals. Panics if `node` is out of range.
    pub fn street(&self, node: u32) -> Street {
        self.tree.node(self.tree_node(node)).street()
    }

    /// The children of the public node `node`: one per action at a player node, one per
    /// outcome at a chance node, and none at a terminal. Panics if `node` is out of range.
    pub fn children(&self, node: u32) -> &[u32] {
        match &self.public(node).kind {
            Kind::Player { children, .. } | Kind::Chance { children, .. } => children,
            Kind::Terminal => &[],
        }
    }

    /// The acting player at the public node `node`, or `None` if it is not a player node.
    /// Panics if `node` is out of range.
    pub fn actor(&self, node: u32) -> Option<Player> {
        match self.public(node).kind {
            Kind::Player { actor, .. } => Some(actor),
            _ => None,
        }
    }

    /// The actions at the public node `node`, its betting node's, or none if it is not a player
    /// node. Panics if `node` is out of range.
    pub fn actions(&self, node: u32) -> &[Action] {
        match self.tree.node(self.tree_node(node)) {
            Node::Player { actions, .. } => actions,
            _ => &[],
        }
    }

    /// At a chance node, the card each outcome deals, by outcome index; none at another node.
    /// Panics if `node` is out of range.
    pub fn outcomes(&self, node: u32) -> &[Card] {
        match &self.public(node).kind {
            Kind::Chance { cards, .. } => cards,
            _ => &[],
        }
    }

    /// The child of the chance node `node` that deals `card`, or `None` if `node` is not a
    /// chance node or `card` is on its board. Panics if `node` is out of range.
    pub fn card_child(&self, node: u32, card: Card) -> Option<u32> {
        let Kind::Chance {
            cards, children, ..
        } = &self.public(node).kind
        else {
            return None;
        };
        let outcome = cards.binary_search(&card).ok()?;
        Some(children[outcome])
    }

    /// The node that `path` leads to from the root, as the
    /// [module documentation](self#node-paths) describes paths: tokens separated by any
    /// whitespace, such as `x b50 c 7s x`, with `""` for the root.
    ///
    /// Fails at the first token that does not lead on: one that is not an action at the player
    /// node the tokens before it reach, which the error lists the actions of; one at a chance
    /// node that is not a card, or is a card already on the board; or one that comes after the
    /// end of the hand.
    pub fn find(&self, path: &str) -> Result<u32, NodePathError> {
        let mut at = 0u32;
        let mut before: Vec<String> = Vec::new();
        for token in path.split_whitespace() {
            let found = match &self.public(at).kind {
                Kind::Player { children, .. } => {
                    let actions = self.actions(at);
                    match actions.iter().position(|a| a.to_string() == token) {
                        Some(action) => Ok(children[action]),
                        None => Err(NodePathErrorKind::NoAction {
                            actions: actions.iter().map(Action::to_string).collect(),
                        }),
                    }
                }
                Kind::Chance { .. } => {
                    let street = self.street(at);
                    match token.parse::<Card>() {
                        Err(_) => Err(NodePathErrorKind::NotACard { street }),
                        Ok(card) => self.card_child(at, card).ok_or(NodePathErrorKind::OnBoard {
                            card,
                            board: self.board(at),
                        }),
                    }
                }
                Kind::Terminal => {
                    let Node::Terminal { outcome, .. } = self.tree.node(self.tree_node(at)) else {
                        unreachable!("a public terminal is a betting terminal");
                    };
                    Err(NodePathErrorKind::HandOver { outcome: *outcome })
                }
            };
            match found {
                Ok(child) => at = child,
                Err(kind) => {
                    return Err(NodePathError {
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

    /// The path of `node`, as [`PostflopGame::find`] reads it: its tokens joined by single
    /// spaces, and `""` for the root. Panics if `node` is out of range.
    pub fn node_path(&self, node: u32) -> String {
        let tokens: Vec<String> = self
            .path(node)
            .into_iter()
            .map(|(at, index)| match &self.public(at).kind {
                Kind::Player { .. } => self.actions(at)[index].to_string(),
                Kind::Chance { cards, .. } => cards[index].to_string(),
                Kind::Terminal => unreachable!("a path passes through no terminal"),
            })
            .collect();
        tokens.join(" ")
    }

    /// The steps from the root to `node`, as pairs of a node on the way and the index of the
    /// action taken there, at a player node, or of the outcome, at a chance node.
    ///
    /// Panics if `node` is out of range.
    pub fn path(&self, node: u32) -> Vec<(u32, usize)> {
        let num_nodes = self.nodes.len();
        assert!(
            (node as usize) < num_nodes,
            "node {node} is out of range for {num_nodes} nodes"
        );
        let mut path = Vec::new();
        let mut at = 0;
        while at != node {
            // Every subtree takes a contiguous range of indices in pre-order, children in
            // order, so `node` lies below the last child that comes at or before it.
            let children = self.children(at);
            let index = children
                .iter()
                .rposition(|&child| child <= node)
                .expect("the public tree is in pre-order");
            path.push((at, index));
            at = children[index];
        }
        path
    }

    /// Each player's own reach of `node` under `profile`, by hand: the product of the
    /// probabilities of its own actions on the way from the root, and 0 for a hand that holds a
    /// card dealt on the way. At the root every hand's is 1.
    ///
    /// Panics if `node` is out of range, or if `profile` has no strategy, or one of the wrong
    /// size, at a player node on the way.
    pub fn reach(&self, profile: &PublicProfile, node: u32) -> [Vec<f64>; 2] {
        let mut reach = [0, 1].map(|player| vec![1.0; self.num_hands(player)]);
        for (at, index) in self.path(node) {
            match &self.public(at).kind {
                Kind::Player { actor, children } => {
                    let own = &mut reach[actor.index()];
                    let num_hands = own.len();
                    let strategy = self.strategy_at(profile, at, children.len(), num_hands);
                    let probabilities = &strategy[index * num_hands..(index + 1) * num_hands];
                    for (reach, &p) in own.iter_mut().zip(probabilities) {
                        *reach *= f64::from(p);
                    }
                }
                Kind::Chance { cards, .. } => {
                    let dealt = CardSet::single(cards[index]);
                    for player in [Player::Oop, Player::Ip] {
                        let combos = self.hands(player).combos();
                        for (reach, combo) in reach[player.index()].iter_mut().zip(combos) {
                            if combo.intersects(dealt) {
                                *reach = 0.0;
                            }
                        }
                    }
                }
                Kind::Terminal => unreachable!("a path passes through no terminal"),
            }
        }
        reach
    }

    /// At a player node, the probability that the acting player holds each of its hands once
    /// the node is reached under `profile`, as the [module documentation](self) defines it. The
    /// weights sum to 1.
    ///
    /// `None` if `node` is not a player node, or if `profile` reaches it with probability zero.
    /// Panics as [`PostflopGame::reach`] does.
    pub fn hand_weights(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let actor = self.actor(node)?;
        let (me, them) = (actor.index(), actor.opponent().index());
        let reach = self.reach(profile, node);
        let opponent: Vec<f64> = reach[them]
            .iter()
            .zip(&self.deal[them])
            .map(|(r, d)| r * d)
            .collect();
        let mut met = vec![0.0; self.num_hands(me)];
        self.board_hands()
            .fold_values(actor, &opponent, 1.0, &mut met);
        let mut weights: Vec<f64> = self.deal[me]
            .iter()
            .zip(&reach[me])
            .zip(&met)
            .map(|((d, r), m)| d * r * m)
            .collect();
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        for weight in &mut weights {
            *weight /= total;
        }
        Some(weights)
    }

    /// At a player node, the probability of each action once the node is reached under
    /// `profile`: `Σ_h P(h | n) · σ(a | h)` over the acting player's hands, with the weights of
    /// [`PostflopGame::hand_weights`], card removal by the opponent's range included. This is
    /// how often each action is actually taken when play reaches the node. It is not what a
    /// range view shows, which [`PostflopGame::range_view_frequencies`] gives: see the
    /// [module documentation](self#ranges-at-a-node).
    ///
    /// `None` if `node` is not a player node, or if `profile` reaches it with probability zero.
    /// Panics as [`PostflopGame::reach`] does, or if `profile` has no strategy at `node`.
    pub fn action_frequencies(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let weights = self.hand_weights(profile, node)?;
        Some(self.frequencies_with(profile, node, &weights))
    }

    /// At a player node, each hand's share of the acting player's range there by its own
    /// combos alone: its range weight times its own reach of the node under `profile`,
    /// normalised to sum to 1. The opponent's range does not enter, so there is no card removal
    /// by it. This is the weighting of a solver's range view: see the
    /// [module documentation](self#ranges-at-a-node).
    ///
    /// `None` if `node` is not a player node, or if the acting player's own reach of it is zero
    /// with every hand. Panics as [`PostflopGame::reach`] does.
    pub fn range_view_weights(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let me = self.actor(node)?.index();
        let reach = self.reach(profile, node);
        // The deal is the range weight over √Z, which the normalisation cancels.
        let mut weights: Vec<f64> = self.deal[me]
            .iter()
            .zip(&reach[me])
            .map(|(d, r)| d * r)
            .collect();
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        for weight in &mut weights {
            *weight /= total;
        }
        Some(weights)
    }

    /// At a player node, each action's frequency as a range view shows it:
    /// `Σ_h W(h | n) · σ(a | h)` with the weights of [`PostflopGame::range_view_weights`], the
    /// acting player's own combos weighted by range weight and own reach, with no card removal
    /// by the opponent's range. Use it to compare a strategy with another solver's range view.
    /// For how often each action is actually taken at the node, use
    /// [`PostflopGame::action_frequencies`].
    ///
    /// `None` if `node` is not a player node, or if the acting player's own reach of it is zero
    /// with every hand. Panics as [`PostflopGame::reach`] does, or if `profile` has no strategy
    /// at `node`.
    pub fn range_view_frequencies(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let weights = self.range_view_weights(profile, node)?;
        Some(self.frequencies_with(profile, node, &weights))
    }

    /// At a player node, each hand class in the acting player's range there, in grid order:
    /// its share of the range and its frequency for each action, both as a range view weighs
    /// them ([`PostflopGame::range_view_weights`]), as the
    /// [module documentation](self#ranges-at-a-node) defines them. A class is in the range when
    /// one of its hands has a positive weight there; the other classes are left out.
    ///
    /// `None` if `node` is not a player node, or if the acting player's own reach of it is zero
    /// with every hand. Panics as [`PostflopGame::reach`] does, or if `profile` has no strategy
    /// at `node`.
    pub fn range_view_classes(&self, profile: &PublicProfile, node: u32) -> Option<Vec<ClassView>> {
        let weights = self.range_view_weights(profile, node)?;
        let actor = self
            .actor(node)
            .expect("the weights are None off a player node");
        let num_actions = self.children(node).len();
        let combos = self.hands(actor).combos();
        let strategy = self.strategy_at(profile, node, num_actions, combos.len());
        let mut by_class: BTreeMap<HandClass, Vec<usize>> = BTreeMap::new();
        for (hand, &combo) in combos.iter().enumerate() {
            by_class.entry(HandClass::of(combo)).or_default().push(hand);
        }
        let classes = by_class
            .into_iter()
            .filter_map(|(class, hands)| {
                let share: f64 = hands.iter().map(|&h| weights[h]).sum();
                if share <= 0.0 {
                    return None;
                }
                let frequencies = (0..num_actions)
                    .map(|a| {
                        let total: f64 = hands
                            .iter()
                            .map(|&h| weights[h] * f64::from(strategy[a * combos.len() + h]))
                            .sum();
                        total / share
                    })
                    .collect();
                Some(ClassView {
                    class,
                    hands,
                    share,
                    frequencies,
                })
            })
            .collect();
        Some(classes)
    }

    /// The probability that a hand dealt plays to `node` under `profile`:
    /// `Σ deal(0)[h] · π_0(n, h) · deal(1)[o] · π_1(n, o)` over the pairs of hands that share no
    /// card, times the probability of each card dealt on the way. It is 1 at the root, up to
    /// rounding, and it is defined at every node.
    ///
    /// Panics as [`PostflopGame::reach`] does.
    pub fn reach_probability(&self, profile: &PublicProfile, node: u32) -> f64 {
        let reach = self.reach(profile, node);
        let ip: Vec<f64> = reach[1]
            .iter()
            .zip(&self.deal[1])
            .map(|(r, d)| r * d)
            .collect();
        let mut met = vec![0.0; reach[0].len()];
        self.board_hands()
            .fold_values(Player::Oop, &ip, 1.0, &mut met);
        let pairs: f64 = self.deal[0]
            .iter()
            .zip(&reach[0])
            .zip(&met)
            .map(|((d, r), m)| d * r * m)
            .sum();
        let dealt: f64 = self
            .path(node)
            .into_iter()
            .filter_map(|(at, _)| match self.public(at).kind {
                Kind::Chance { probability, .. } => Some(probability),
                _ => None,
            })
            .product();
        pairs * dealt
    }

    /// `Σ_h weights[h] · σ(a | h)` for each action at the player node `node`.
    fn frequencies_with(&self, profile: &PublicProfile, node: u32, weights: &[f64]) -> Vec<f64> {
        let num_actions = self.children(node).len();
        let strategy = self.strategy_at(profile, node, num_actions, weights.len());
        strategy
            .chunks_exact(weights.len())
            .map(|probabilities| {
                probabilities
                    .iter()
                    .zip(weights)
                    .map(|(&p, &w)| f64::from(p) * w)
                    .sum()
            })
            .collect()
    }

    /// The strategy `profile` holds at the player node `node`, checked against its shape.
    fn strategy_at<'a>(
        &self,
        profile: &'a PublicProfile,
        node: u32,
        num_actions: usize,
        num_hands: usize,
    ) -> &'a [f32] {
        let strategy = profile
            .get(node as usize)
            .unwrap_or_else(|| panic!("the profile has no strategy at node {node}"));
        assert_eq!(
            strategy.len(),
            num_actions * num_hands,
            "the profile's strategy at node {node} has the wrong size"
        );
        strategy
    }

    fn public(&self, node: u32) -> &PublicTreeNode {
        let num_nodes = self.nodes.len();
        self.nodes
            .get(node as usize)
            .unwrap_or_else(|| panic!("node {node} is out of range for {num_nodes} nodes"))
    }
}

/// A betting node still to expand on a board, and the public parent and child slot that should
/// point at it.
struct Pending {
    at: u32,
    board: u32,
    parent: Option<(u32, usize)>,
}

/// The public tree of `tree`, which has `size` nodes, and the boards it reaches, the spot's
/// first. Built in pre-order with an explicit stack, as the betting tree is, because betting
/// lines can be longer than the call stack allows.
fn expand(
    tree: &PostflopTree,
    spot: &Spot,
    root: BoardHands,
    size: usize,
) -> (Vec<PublicTreeNode>, Vec<Board>) {
    let mut boards = vec![Board {
        hands: root,
        to_root: [Vec::new(), Vec::new()],
    }];
    let mut board_of: HashMap<CardSet, u32> = HashMap::from([(spot.board, 0)]);
    let mut nodes: Vec<PublicTreeNode> = Vec::with_capacity(size);
    let mut work = vec![Pending {
        at: PostflopTree::ROOT,
        board: 0,
        parent: None,
    }];
    while let Some(Pending { at, board, parent }) = work.pop() {
        let index = u32::try_from(nodes.len()).expect("the node limit keeps indices in a u32");
        if let Some((parent, slot)) = parent {
            match &mut nodes[parent as usize].kind {
                Kind::Player { children, .. } | Kind::Chance { children, .. } => {
                    children[slot] = index;
                }
                Kind::Terminal => unreachable!("a terminal has no children"),
            }
        }
        let kind = match tree.node(at) {
            Node::Player {
                actor, children, ..
            } => {
                // Pushed in reverse, so that the first child's subtree is made first.
                for (slot, &child) in children.iter().enumerate().rev() {
                    work.push(Pending {
                        at: child,
                        board,
                        parent: Some((index, slot)),
                    });
                }
                Kind::Player {
                    actor: *actor,
                    children: vec![0; children.len()],
                }
            }
            Node::Chance { child, .. } => {
                let on_board = boards[board as usize].hands.board();
                let cards: Vec<Card> = (CardSet::FULL - on_board).iter().collect();
                for (slot, &card) in cards.iter().enumerate().rev() {
                    let dealt = on_board | CardSet::single(card);
                    let next = *board_of.entry(dealt).or_insert_with(|| {
                        let next = Board::new(dealt, spot, &boards[0].hands);
                        boards.push(next);
                        (boards.len() - 1) as u32
                    });
                    work.push(Pending {
                        at: *child,
                        board: next,
                        parent: Some((index, slot)),
                    });
                }
                // Both players hold two cards each that are not on the board.
                let unseen = cards.len() - 4;
                Kind::Chance {
                    children: vec![0; cards.len()],
                    cards,
                    probability: 1.0 / unseen as f64,
                }
            }
            Node::Terminal { .. } => Kind::Terminal,
        };
        nodes.push(PublicTreeNode {
            tree: at,
            board,
            kind,
        });
    }
    debug_assert_eq!(nodes.len(), size, "the count of the public tree is exact");
    (nodes, boards)
}

impl PublicGame for PostflopGame {
    fn num_hands(&self, player: usize) -> usize {
        self.deal[player].len()
    }

    fn deal(&self, player: usize) -> &[f64] {
        &self.deal[player]
    }

    fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    fn root(&self) -> usize {
        PostflopTree::ROOT as usize
    }

    fn node(&self, node: usize) -> PublicNode {
        match &self.nodes[node].kind {
            Kind::Player { actor, children } => PublicNode::Player {
                player: actor.index(),
                num_actions: children.len(),
            },
            Kind::Chance { children, .. } => PublicNode::Chance {
                num_outcomes: children.len(),
            },
            Kind::Terminal => PublicNode::Terminal,
        }
    }

    fn child(&self, node: usize, index: usize) -> usize {
        match &self.nodes[node].kind {
            Kind::Player { children, .. } | Kind::Chance { children, .. } => {
                children[index] as usize
            }
            Kind::Terminal => panic!("node {node} is a terminal, with no children"),
        }
    }

    fn outcome_probability(&self, node: usize, _outcome: usize) -> f64 {
        match self.nodes[node].kind {
            Kind::Chance { probability, .. } => probability,
            _ => panic!("node {node} is not a chance node"),
        }
    }

    fn rules_out(&self, node: usize, outcome: usize, player: usize, hand: usize) -> bool {
        let Kind::Chance { cards, .. } = &self.nodes[node].kind else {
            panic!("node {node} is not a chance node");
        };
        let combo = self.hands(player_of(player)).combos()[hand];
        combo.intersects(CardSet::single(cards[outcome]))
    }

    fn terminal_values(
        &self,
        node: usize,
        player: usize,
        opponent_reach: &[f64],
        values: &mut [f64],
    ) {
        let public = &self.nodes[node];
        let payoff = self.payoffs[public.tree as usize]
            .unwrap_or_else(|| panic!("node {node} is not a terminal"));
        let traverser = player_of(player);
        let board = &self.boards[public.board as usize];
        if public.board == 0 {
            evaluate(&board.hands, traverser, payoff, opponent_reach, values);
            return;
        }
        // A later board's lists hold only the hands that miss it: gather the opponent's reach
        // into its order and write the values back.
        assert_eq!(
            opponent_reach.len(),
            self.num_hands(1 - player),
            "the reach needs one entry per opponent hand"
        );
        let (mine, theirs) = (&board.to_root[player], &board.to_root[1 - player]);
        let reach: Vec<f64> = theirs.iter().map(|&o| opponent_reach[o as usize]).collect();
        let mut local = vec![0.0; mine.len()];
        evaluate(&board.hands, traverser, payoff, &reach, &mut local);
        values.fill(0.0);
        for (&hand, value) in mine.iter().zip(local) {
            values[hand as usize] = value;
        }
    }
}

/// The traverser's values at a terminal with `payoff`, on the board of `hands`.
fn evaluate(
    hands: &BoardHands,
    traverser: Player,
    payoff: Payoff,
    reach: &[f64],
    values: &mut [f64],
) {
    match payoff {
        Payoff::Fold(payoffs) => {
            hands.fold_values(traverser, reach, payoffs[traverser.index()], values);
        }
        Payoff::Showdown(payoff) => hands.showdown_values(traverser, reach, payoff, values),
    }
}

/// Why a node path does not lead to a node: see [`PostflopGame::find`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePathError {
    /// The tokens before the one that failed, which lead to a node.
    pub before: Vec<String>,
    /// The token that failed.
    pub token: String,
    pub kind: NodePathErrorKind,
}

/// What is at the node where a path fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodePathErrorKind {
    /// A player node with no action of that token. `actions` are its tokens, in order.
    NoAction { actions: Vec<String> },
    /// A terminal: the hand is over, and no token can follow.
    HandOver { outcome: Outcome },
    /// A chance node, which deals `street`, and a token that is not a card.
    NotACard { street: Street },
    /// A chance node, and a card that is already on its board.
    OnBoard { card: Card, board: CardSet },
}

/// One line, such as "`b75` is not an action after `x`; the actions there are `x` and `b50`".
impl fmt::Display for NodePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (token, before) = (&self.token, self.before.join(" "));
        match &self.kind {
            NodePathErrorKind::NoAction { actions } => {
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
            NodePathErrorKind::HandOver { outcome } => {
                let end = match outcome {
                    Outcome::Fold { winner } => format!("where {} folds", winner.opponent()),
                    Outcome::Showdown => "a showdown".to_string(),
                };
                write!(
                    f,
                    "`{token}` comes after the end of the hand, at `{before}`, {end}"
                )
            }
            NodePathErrorKind::NotACard { street } => write!(
                f,
                "`{token}` is not a card; `{before}` deals the {street}, so the next token is the \
                 card that falls, such as `7s`"
            ),
            NodePathErrorKind::OnBoard { card, board } => write!(
                f,
                "`{token}` cannot fall after `{before}`: the {card} is on the board, {board}"
            ),
        }
    }
}

impl std::error::Error for NodePathError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Spot {
        let path = format!("{}/tests/data/{name}.toml", env!("CARGO_MANIFEST_DIR"));
        Spot::load(path).unwrap()
    }

    #[test]
    fn every_board_is_built_once_and_indexed_against_the_spots_hands() {
        // On the flop spot every turn and every river can be reached, by checking through: one
        // board for the flop, 49 turns and C(49, 2) = 1,176 rivers, each river shared by the
        // two orders its cards can fall in.
        let game = PostflopGame::from_spot(&load("flop_small")).unwrap();
        assert_eq!(game.boards.len(), 1 + 49 + 1176);
        let mut seen = std::collections::HashSet::new();
        for board in &game.boards {
            assert!(seen.insert(board.hands.board()), "a board is built twice");
        }
        for board in &game.boards[1..] {
            for player in [Player::Oop, Player::Ip] {
                let list = board.hands.hands(player);
                let root = game.hands(player);
                assert_eq!(board.to_root[player.index()].len(), list.len());
                for (&hand, &combo) in board.to_root[player.index()].iter().zip(list.combos()) {
                    assert_eq!(root.combos()[hand as usize], combo);
                    assert!(!combo.intersects(board.hands.board()));
                }
                // Every hand of the spot's that misses this board is in its list.
                let misses = root
                    .combos()
                    .iter()
                    .filter(|c| !c.intersects(board.hands.board()))
                    .count();
                assert_eq!(misses, list.len());
            }
        }
        let turn = game.find("x x 8h x x 3d").unwrap();
        let other = game.find("x x 3d x x 8h").unwrap();
        assert_eq!(
            game.nodes[turn as usize].board,
            game.nodes[other as usize].board
        );
        assert_ne!(turn, other);
    }

    #[test]
    fn the_count_is_the_built_size() {
        for name in ["flop_small", "turn_small"] {
            let game = PostflopGame::from_spot(&load(name)).unwrap();
            assert_eq!(public_size(game.tree()), game.nodes.len() as u64);
        }
        // A river spot's public tree is its betting tree, on one board.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/spots/river/four_flush.toml"
        );
        let game = PostflopGame::new(&Spot::load(path).unwrap()).unwrap();
        assert_eq!(game.nodes.len(), game.tree().nodes().len());
        assert_eq!(game.boards.len(), 1);
        assert!(game.boards[0].to_root.iter().all(Vec::is_empty));
    }

    #[test]
    fn the_chance_of_a_card_is_one_in_the_cards_neither_player_holds() {
        let game = PostflopGame::from_spot(&load("flop_small")).unwrap();
        let turn = game.find("x x").unwrap();
        let river = game.find("x x 8h x x").unwrap();
        assert_eq!(game.outcome_probability(turn as usize, 0), 1.0 / 45.0);
        assert_eq!(game.outcome_probability(river as usize, 0), 1.0 / 44.0);
        assert_eq!(game.outcomes(turn).len(), 49);
        assert_eq!(game.outcomes(river).len(), 48);
    }
}
