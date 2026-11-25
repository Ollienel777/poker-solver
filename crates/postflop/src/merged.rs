//! The merged game: a spot's game with each class of turn or river cards that a symmetry of the
//! suits makes alike dealt once, which [`VectorCfr`] iterates on in place of the full game.
//!
//! # When two cards are merged
//!
//! Write `S` for the spot's board and `Γ` for the relabellings of the suits ([`SuitPermutation`])
//! that keep the spot's deal:
//!
//! 1. they map `S` onto itself as a set, and
//! 2. they leave both players' hand lists as they are, weights included: in each range, every
//!    combo that misses `S` has the weight of its image.
//!
//! That is [`DealtBoard::symmetries_with_ranges`] on the spot's board, [`merging_group`], and it
//! is a group. At a chance node whose board as dealt is `B`, the spot's board and then the cards
//! dealt since, one a street, the merging uses `Γ_B`: the members of `Γ` that also keep each card
//! dealt since the spot began ([`DealtBoard::symmetries`]). Two cards that can fall there are
//! *merged* when a member of `Γ_B` maps one onto the other. They are then one class
//! ([`DealtBoard::next_cards_within`]), dealt once, by its lowest card, the *representative*;
//! each other card `c'` of the class is an *image* of it, through the member `σ` of `Γ_B` that
//! maps `c'` to the representative ([`NextCards::to_representative`]).
//!
//! # Why that is sound
//!
//! A member `σ` of `Γ` is an automorphism of the game. Relabel every card dealt on every path by
//! `σ`, and every hand: the betting tree is the same, the deal is the same (condition 2, and two
//! combos share a card exactly when their images do), each chance node deals the cards off its
//! board, each with probability `1/(48 − b)`, and each terminal pays the same, since a fold pays by
//! the chips and a showdown by the ranks of the hands, which a relabelling keeps. So `σ` maps
//! public node `n`, reached by a path, to the node `σ(n)` reached by the relabelled path, and hand
//! `h` to hand `σ(h)`, and the game after the relabelling is the game. CFR is equivariant under
//! such a map: one iteration computes every stored value from the game and the stored values by
//! the same formulas at every node, so when the state at `(σ(n), σ(h))` is the state at `(n, h)`
//! before an iteration, it is so after it, and from zero it is so at the start. In exact
//! arithmetic, then, the full game's solver gives `(σ(n), σ(h))` what it gives `(n, h)` after
//! every iteration: the same regrets, strategy sums, strategies, reaches and values.
//!
//! At a chance node `n` on `B`, a member `σ` of `Γ_B` keeps every card on the way to `n`, so
//! `σ(n) = n`, and it maps the subtree after card `c` onto the subtree after `σ(c)`. For an image
//! `c'` with representative `r = σ(c')`, the values after `c'` are those after `r` with the hands
//! relabelled, `q(n·c', h) = q(n·r, σ(h))`, and the regrets and strategy sums after `c'` are a
//! relabelled copy of those after `r`, provided the reach entering `n` is the same for each
//! hand `o` as for `σ(o)`, the opponent's for the values and the traverser's own for the sums.
//! So the merged game walks the subtree after `r` once, adds for each image the values read
//! through `σ` (the images of [`pk_cfr::vector`](pk_cfr::vector#images)), and stores nothing for
//! the subtree after `c'`. The condition on the reach is the solver's state above `n` being
//! symmetric, which the next section shows the merged solver keeps exactly. Then in exact
//! arithmetic its iteration is the full game's, whatever the state it starts from. In floating
//! point it is not bit for bit: the full game evaluates the subtrees after `r` and after `c'` with
//! the hands in different orders, which round differently, so the two agree within rounding.
//!
//! # Why the merged solver stays symmetric
//!
//! A merged node `m` on `B` as dealt is kept by its *stabilizer*, `Γ_B`, the members of `Γ` that
//! keep each street of `B` ([`MergedGame::stabilizer`]), which map its subtree onto itself. In
//! exact arithmetic the solver's values at `m` are the same for hand `h` and hand `σ(h)`, for
//! every `σ` in the stabilizer. In floating point they need not be: a terminal's evaluation and a
//! chance node's sum add the terms of `h` and of `σ(h)` in different orders, and where regret
//! matching is at zero a last-bit difference becomes a different strategy, after which the two
//! hands play differently at `m`. The full game's solver does that to itself harmlessly, since it
//! is still CFR on one game. The merged solver cannot: once two such hands play differently
//! above a chance node, an image is read against the opponent's reach `ρ` where the full game has
//! `ρ ∘ σ⁻¹`, and the merged game is no longer the full game, even in exact arithmetic. Left to
//! itself the merged solver does part them: on `monotone_pot` within 5 iterations, and on the
//! test spot `flop_monotone_wide` by iteration 8 under every schedule.
//!
//! So the merged game keeps them together. It gives the CFR walk, at each merged node and for
//! each player, the orbits of the player's hands under the node's stabilizer, each named by its
//! first hand, as the node's [alike hands](pk_cfr::vector#alike-hands), and the walk gives every
//! hand its orbit's first hand's value at every terminal and chance node. The conditions the walk
//! needs hold: the deal is the same for hands that `Γ`, the root's stabilizer, maps onto one
//! another; a player node's children are on its board and have its stabilizer; and a chance
//! node's child after card `r` has the members of the node's stabilizer that keep `r`, whose
//! orbits lie within the node's, and which map the hands holding `r` onto themselves. So after
//! every iteration, on any number of threads, the merged solver stores the same values bit for bit
//! for `h` and `σ(h)` at every merged node and every `σ` in its stabilizer, and the reach entering
//! every chance node is symmetric exactly.
//!
//! # Why a relabelling that moves a dealt card is not used
//!
//! `Γ_B` keeps each card dealt since the spot began, not merely the cards on the board as a set.
//! On a flop of 7s 7h 2c with a turn of 7d, swapping hearts and diamonds keeps the four cards,
//! but it maps the history, flop 7s 7h 2c and turn 7d, to flop 7s 7d 2c and turn 7h: a node of a
//! game on another flop, not of this one. The reaches at the river chance node come from the
//! flop's strategies, played seeing 7s 7h 2c, and nothing relates them to that other flop's, so
//! the rivers that the swap maps onto each other are not alike here, and merging them would be
//! wrong. Such a relabelling is never in `Γ`, whose members keep the spot's board as a set and so
//! cannot move a later card onto one of it: within `Γ`, keeping each card dealt and keeping the
//! board as a set come to the same. What must not be done is to take a chance node's
//! relabellings from its board alone, as the stabilizer of its cards: that admits the swap. For
//! the same reason `Γ_B` never grows down a line: a chance node merges under the members that
//! kept every street before it. And condition 2 is on the spot's hand lists, not on the combos
//! still live at a node: the reaches at a river chance node come from the turn's and the flop's
//! strategies, which the hands that hold the turn card played too.
//!
//! # The merged tree
//!
//! The merged game's public tree is the full game's with the subtrees after images left out. Its
//! nodes are in pre-order, a chance node's outcomes are its classes in order of their
//! representatives, each with its images in card order, and every node has its betting node and
//! its board as dealt by the representatives on its path; its terminals are evaluated on the full
//! game's boards. Each player node stores its regrets and strategy sums for every hand of the
//! spot, as the full game's do (DECISIONS P25), so the hand indices are the full game's.
//!
//! Every node `n` of the full game has a *counterpart*, a node `m` of the merged game, and a
//! relabelling `τ` in `Γ` such that `n`'s subtree, each hand `h` written `τ(h)`, is `m`'s
//! ([`MergedGame::counterpart`]). A node on a path of representatives is its own counterpart's,
//! with `τ` the identity. Below a chance node whose counterpart has `τ`, the card `c` is `τ(c)` at
//! the counterpart, whose class has the representative `r` and the relabelling `π` that maps
//! `τ(c)` to `r`, so the child after `c` has the counterpart's child after `r`, with `τ` then `π`.
//!
//! # Measured on the full game
//!
//! The solver iterates on the merged game, and nothing is measured there.
//! [`MergedGame::average_strategy`] expands a solver's average strategy onto the full public
//! tree, each node playing its counterpart's strategy with every hand `h` reading hand `τ(h)`, and
//! the exploitability, the [`Solution`](crate::Solution), its read-outs, the invariants, the dumps
//! and the comparison are those of that profile on the full game, by the code that measures any
//! other, which assumes no symmetry. So every measurement of a merged solve is a check of the
//! merging that does not depend on it.
//!
//! [`MergedGame::new`] gives `None` when `Γ` holds the identity only, as on an unpaired rainbow
//! flop, on a turn of four suits or with a range that names a single combo, or when the spot has
//! no chance node, as on the river. The solve then iterates on the full game, as it does with
//! merging off.

use pk_cfr::{PublicGame, PublicNode, PublicProfile, VectorCfr};
use pk_core::{
    Card, CardSet, DealtBoard, NUM_SUIT_PERMUTATIONS, NextCards, Range, SuitGroup, SuitPermutation,
};
use pk_game::{Node, Player};

use crate::game::{PostflopGame, player_of};
use crate::hands::HandList;

/// `Γ`, the relabellings of the suits that keep a spot's deal, as the
/// [module documentation](self) defines them: the symmetries of the spot's `board` that leave
/// each player's hand list on it, `hands`, as it is, weights included.
pub fn merging_group(board: CardSet, hands: [&HandList; 2]) -> SuitGroup {
    let ranges = hands.map(|list| {
        let mut range = Range::empty();
        for (&combo, &weight) in list.combos().iter().zip(list.weights()) {
            // The weights are the range's `f32`s, widened exactly.
            range.set(combo, weight as f32);
        }
        range
    });
    DealtBoard::new(board)
        .expect("a spot's board has 3 to 5 cards")
        .symmetries_with_ranges(&ranges)
}

/// A spot's game with each class of cards that a symmetry makes alike dealt once, as the
/// [module documentation](self) describes it. It borrows the full game, whose boards, hands,
/// deal and payoffs it shares.
#[derive(Debug, Clone)]
pub struct MergedGame<'a> {
    full: &'a PostflopGame,
    /// `Γ`.
    group: SuitGroup,
    /// The members of `Γ` in index order. A relabelling is stored as its place here.
    members: Vec<SuitPermutation>,
    /// The merged tree, in pre-order from the root at 0.
    nodes: Vec<MergedNode>,
    /// The chance nodes' outcomes and images, by [`MergedKind::Chance`]'s index.
    chances: Vec<MergedChance>,
    /// By node of the full game: its counterpart and `τ`.
    counterparts: Vec<Counterpart>,
    /// For each player, then for each member `σ` of `Γ`: for each hand `h`, the hand `σ(h)`.
    relabelled: [Vec<Vec<u32>>; 2],
    /// The stabilizers of the merged nodes, each once, by [`MergedNode::stabilizer`].
    stabilizers: Vec<Stabilizer>,
}

/// The members of `Γ` that keep a merged node, its *stabilizer*, with the classes of hands they
/// make alike there.
#[derive(Debug, Clone)]
struct Stabilizer {
    group: SuitGroup,
    /// For each player, for each hand, the first hand it is mapped to by a member: empty when the
    /// group holds the identity alone.
    alike: [Vec<u32>; 2],
}

/// A node of the full game's counterpart in the merged game, and the place of `τ` in the
/// members of `Γ`.
#[derive(Debug, Clone, Copy)]
struct Counterpart {
    node: u32,
    relabelling: u8,
}

#[derive(Debug, Clone)]
struct MergedNode {
    /// The betting-tree node.
    tree: u32,
    /// The board here, as an index into the full game's boards.
    board: u32,
    /// The node's stabilizer, as an index into [`MergedGame::stabilizers`].
    stabilizer: u8,
    kind: MergedKind,
}

#[derive(Debug, Clone)]
enum MergedKind {
    /// `children[a]` follows action `a` of the betting node.
    Player {
        actor: Player,
        children: Vec<u32>,
    },
    /// The chance node's outcomes and images, `chances[index]`.
    Chance(u32),
    Terminal,
}

/// A chance node of the merged tree.
#[derive(Debug, Clone)]
struct MergedChance {
    /// The representative each outcome deals, in card order.
    cards: Vec<Card>,
    /// `children[o]` follows outcome `o`.
    children: Vec<u32>,
    probability: f64,
    /// The images of each outcome, in card order: those of outcome `o` are
    /// `images[starts[o]..starts[o + 1]]`.
    images: Vec<Image>,
    starts: Vec<u32>,
}

/// An image of an outcome: its card, and the place in the members of `Γ` of the relabelling
/// that maps it to the outcome's card.
#[derive(Debug, Clone, Copy)]
struct Image {
    card: Card,
    relabelling: u8,
}

/// The size of a merged node's record, of a chance node's, of an image and of a counterpart,
/// for the [memory estimate](crate::memory).
pub(crate) const MERGED_NODE_BYTES: usize = std::mem::size_of::<MergedNode>();
pub(crate) const MERGED_CHANCE_BYTES: usize = std::mem::size_of::<MergedChance>();
pub(crate) const IMAGE_BYTES: usize = std::mem::size_of::<Image>();
pub(crate) const COUNTERPART_BYTES: usize = std::mem::size_of::<Counterpart>();
pub(crate) const STABILIZER_BYTES: usize = std::mem::size_of::<Stabilizer>();

/// A node of the full game still to visit, with what its counterpart is and how to reach it.
struct Pending {
    full: u32,
    /// The board as dealt at the counterpart.
    board: DealtBoard,
    /// `τ`, as its place in the members of `Γ`.
    relabelling: u8,
    place: Place,
}

/// Where a full node's counterpart is.
enum Place {
    /// A node of the merged tree still to make: the root, or the child `slot` of `parent`. The
    /// full node is on a path of representatives.
    New(Option<(u32, usize)>),
    /// A node made already.
    Made(u32),
    /// The child after outcome `outcome` of the merged chance node `chance`, which is made
    /// before the node is visited: an outcome's representative is its lowest card, so the full
    /// node after it comes before the full nodes after its images in pre-order.
    After { chance: u32, outcome: usize },
}

impl<'a> MergedGame<'a> {
    /// The merged game of `full`, or `None` when it would be the full game: when `Γ` holds the
    /// identity only, or when the full game has no chance node.
    pub fn new(full: &'a PostflopGame) -> Option<MergedGame<'a>> {
        let has_chance = full
            .tree()
            .nodes()
            .iter()
            .any(|node| matches!(node, Node::Chance { .. }));
        let start = full.board(0);
        let group = merging_group(start, [full.hands(Player::Oop), full.hands(Player::Ip)]);
        if group.is_trivial() || !has_chance {
            return None;
        }
        let members: Vec<SuitPermutation> = group.iter().collect();
        let mut place_of = [u8::MAX; NUM_SUIT_PERMUTATIONS];
        for (place, member) in members.iter().enumerate() {
            place_of[member.index()] = place as u8;
        }
        let relabelled = [Player::Oop, Player::Ip].map(|player| {
            let list = full.hands(player);
            members
                .iter()
                .map(|member| {
                    list.combos()
                        .iter()
                        .map(|&combo| {
                            let image = list
                                .position(member.apply_combo(combo))
                                .expect("a relabelling that keeps the deal keeps the hand list");
                            image as u32
                        })
                        .collect()
                })
                .collect()
        });
        let mut game = MergedGame {
            full,
            group,
            members,
            nodes: Vec::new(),
            chances: Vec::new(),
            counterparts: Vec::with_capacity(full.num_nodes()),
            relabelled,
            stabilizers: Vec::new(),
        };
        game.build(&place_of);
        Some(game)
    }

    /// Walks the full tree in pre-order, making each node of the merged tree when the walk
    /// reaches the full node on a path of representatives, and recording every full node's
    /// counterpart. Built with an explicit stack, as the full tree is.
    fn build(&mut self, place_of: &[u8; NUM_SUIT_PERMUTATIONS]) {
        let full = self.full;
        let identity = place_of[SuitPermutation::IDENTITY.index()];
        let mut work = vec![Pending {
            full: 0,
            board: DealtBoard::new(full.board(0)).expect("a spot's board has 3 to 5 cards"),
            relabelling: identity,
            place: Place::New(None),
        }];
        while let Some(Pending {
            full: at,
            board,
            relabelling,
            place,
        }) = work.pop()
        {
            debug_assert_eq!(
                self.counterparts.len(),
                at as usize,
                "the walk is in pre-order"
            );
            let new = matches!(place, Place::New(_));
            let node = match place {
                Place::New(parent) => self.make(at, parent, board),
                Place::Made(node) => node,
                Place::After { chance, outcome } => {
                    let MergedKind::Chance(chance) = self.nodes[chance as usize].kind else {
                        unreachable!("the parent of an image's subtree is a chance node");
                    };
                    self.chances[chance as usize].children[outcome]
                }
            };
            self.counterparts.push(Counterpart { node, relabelling });
            let tau = self.members[relabelling as usize];
            debug_assert_eq!(
                tau.apply_set(full.board(at)),
                self.board(node),
                "the counterpart's board is the full node's relabelled"
            );
            debug_assert_eq!(self.nodes[node as usize].tree, full.tree_node(at));
            let children = full.children(at);
            match full.node(at as usize) {
                PublicNode::Player { .. } => {
                    for (slot, &child) in children.iter().enumerate().rev() {
                        let place = if new {
                            Place::New(Some((node, slot)))
                        } else {
                            Place::Made(self.child(node, slot))
                        };
                        work.push(Pending {
                            full: child,
                            board,
                            relabelling,
                            place,
                        });
                    }
                }
                PublicNode::Chance { .. } => {
                    let next = board.next_cards_within(self.group);
                    if new {
                        self.make_chance(at, &next, place_of);
                    }
                    for (slot, &card) in full.outcomes(at).iter().enumerate().rev() {
                        let seen = tau.apply_card(card);
                        let class = next
                            .class_of(seen)
                            .expect("a card off the board has a class");
                        let representative = class.representative();
                        let outcome = next
                            .classes()
                            .binary_search_by_key(&representative, |class| class.representative())
                            .expect("the class is one of the node's");
                        let to_representative = next
                            .to_representative(seen)
                            .expect("a card off the board has a representative");
                        let next_tau = tau.then(to_representative);
                        let place = match (new, next_tau == SuitPermutation::IDENTITY) {
                            (true, true) => Place::New(Some((node, outcome))),
                            (true, false) => Place::After {
                                chance: node,
                                outcome,
                            },
                            (false, _) => Place::Made(self.child(node, outcome)),
                        };
                        work.push(Pending {
                            full: children[slot],
                            board: board
                                .deal(representative)
                                .expect("a representative is off the board"),
                            relabelling: place_of[next_tau.index()],
                            place,
                        });
                    }
                }
                PublicNode::Terminal => {}
            }
        }
        debug_assert_eq!(self.counterparts.len(), full.num_nodes());
    }

    /// Makes the merged node for the full node `at`, as the child `slot` of `parent` or as the
    /// root, on `board` as dealt, with its children and its chance record left to fill in, and
    /// returns its index.
    fn make(&mut self, at: u32, parent: Option<(u32, usize)>, board: DealtBoard) -> u32 {
        let full = self.full;
        let index =
            u32::try_from(self.nodes.len()).expect("the merged tree is no larger than the full");
        if let Some((parent, slot)) = parent {
            match self.nodes[parent as usize].kind {
                MergedKind::Player {
                    ref mut children, ..
                } => children[slot] = index,
                MergedKind::Chance(chance) => self.chances[chance as usize].children[slot] = index,
                MergedKind::Terminal => unreachable!("a terminal has no children"),
            }
        }
        let kind = match full.node(at as usize) {
            PublicNode::Player { num_actions, .. } => MergedKind::Player {
                actor: full.actor(at).expect("a player node has an actor"),
                children: vec![0; num_actions],
            },
            PublicNode::Chance { .. } => {
                MergedKind::Chance(u32::try_from(self.chances.len()).expect("fewer than u32::MAX"))
            }
            PublicNode::Terminal => MergedKind::Terminal,
        };
        let stabilizer = self.stabilizer_of(board);
        self.nodes.push(MergedNode {
            tree: full.tree_node(at),
            board: full.board_index(at),
            stabilizer,
            kind,
        });
        index
    }

    /// The index of the stabilizer of a node on `board` as dealt, the members of `Γ` that keep
    /// each street of it, made with its classes of alike hands if no node has had it before.
    fn stabilizer_of(&mut self, board: DealtBoard) -> u8 {
        let group = board.symmetries().intersection(self.group);
        if let Some(index) = self.stabilizers.iter().position(|s| s.group == group) {
            return index as u8;
        }
        let alike = [0, 1].map(|player| {
            if group.is_trivial() {
                return Vec::new();
            }
            let members: Vec<&Vec<u32>> = self
                .members
                .iter()
                .zip(&self.relabelled[player])
                .filter(|(member, _)| group.contains(**member))
                .map(|(_, relabelled)| relabelled)
                .collect();
            (0..self.full.num_hands(player))
                .map(|hand| {
                    members
                        .iter()
                        .map(|relabelled| relabelled[hand])
                        .min()
                        .expect("a group holds the identity")
                })
                .collect()
        });
        self.stabilizers.push(Stabilizer { group, alike });
        u8::try_from(self.stabilizers.len() - 1).expect("Γ has fewer than 256 subgroups")
    }

    /// Makes the record of the merged chance node just made for the full chance node `at`, from
    /// the classes of the cards that can fall there.
    fn make_chance(&mut self, at: u32, next: &NextCards, place_of: &[u8; NUM_SUIT_PERMUTATIONS]) {
        let classes = next.classes();
        let cards: Vec<Card> = classes.iter().map(|class| class.representative()).collect();
        let mut images = Vec::with_capacity(next.live_cards().len() - classes.len());
        let mut starts = Vec::with_capacity(classes.len() + 1);
        for class in classes {
            starts.push(images.len() as u32);
            for card in class.members() - CardSet::single(class.representative()) {
                let to_representative = next
                    .to_representative(card)
                    .expect("a member of a class has a representative");
                images.push(Image {
                    card,
                    relabelling: place_of[to_representative.index()],
                });
            }
        }
        starts.push(images.len() as u32);
        let probability = self.full.outcome_probability(at as usize, 0);
        self.chances.push(MergedChance {
            children: vec![0; cards.len()],
            cards,
            probability,
            images,
            starts,
        });
    }

    /// The child of a merged node after action or outcome `index`.
    fn child(&self, node: u32, index: usize) -> u32 {
        match &self.nodes[node as usize].kind {
            MergedKind::Player { children, .. } => children[index],
            MergedKind::Chance(chance) => self.chances[*chance as usize].children[index],
            MergedKind::Terminal => panic!("node {node} is a terminal, with no children"),
        }
    }

    fn chance(&self, node: usize) -> &MergedChance {
        match self.nodes[node].kind {
            MergedKind::Chance(chance) => &self.chances[chance as usize],
            _ => panic!("node {node} is not a chance node"),
        }
    }

    /// The full game, which the merged game shares its boards, hands, deal and payoffs with,
    /// and on which everything about a merged solve is measured.
    pub fn full(&self) -> &'a PostflopGame {
        self.full
    }

    /// `Γ`, the relabellings that keep the spot's deal.
    pub fn group(&self) -> SuitGroup {
        self.group
    }

    /// The counterpart of the full game's node `node` and `τ`, as the
    /// [module documentation](self#the-merged-tree) defines them: the full node's subtree, each
    /// hand `h` written `τ(h)`, is the counterpart's. Panics if `node` is out of range.
    pub fn counterpart(&self, node: u32) -> (u32, SuitPermutation) {
        let Counterpart { node, relabelling } = self.counterparts[node as usize];
        (node, self.members[relabelling as usize])
    }

    /// The betting-tree node of the merged node `node`.
    pub fn tree_node(&self, node: u32) -> u32 {
        self.nodes[node as usize].tree
    }

    /// The stabilizer of the merged node `node`: the members of `Γ` that keep each street of its
    /// board as dealt, which map the node's subtree onto itself.
    pub fn stabilizer(&self, node: u32) -> SuitGroup {
        self.stabilizers[self.nodes[node as usize].stabilizer as usize].group
    }

    /// The board at the merged node `node`: the spot's and the representatives dealt on the way.
    pub fn board(&self, node: u32) -> CardSet {
        self.full.board_cards(self.nodes[node as usize].board)
    }

    /// At a merged chance node, the representative each outcome deals, by outcome index; none
    /// at another node.
    pub fn outcomes(&self, node: u32) -> &[Card] {
        match self.nodes[node as usize].kind {
            MergedKind::Chance(chance) => &self.chances[chance as usize].cards,
            _ => &[],
        }
    }

    /// The images of outcome `outcome` of the merged chance node `node`, in card order, each
    /// with the relabelling that maps it to the outcome's card.
    pub fn outcome_images(
        &self,
        node: u32,
        outcome: usize,
    ) -> impl Iterator<Item = (Card, SuitPermutation)> + '_ {
        let chance = self.chance(node as usize);
        let images =
            &chance.images[chance.starts[outcome] as usize..chance.starts[outcome + 1] as usize];
        images
            .iter()
            .map(|image| (image.card, self.members[image.relabelling as usize]))
    }

    /// A profile on the full public tree from a strategy on the merged one: each full player
    /// node plays its counterpart's strategy, each hand `h` the probabilities of hand `τ(h)`.
    /// `strategy(node, out)` writes the strategy at the merged player node `node`, laid out
    /// `[actions × hands]`, into `out`, which it must fill.
    pub fn expand(&self, mut strategy: impl FnMut(usize, &mut [f32])) -> PublicProfile {
        let full = self.full;
        let mut profile = PublicProfile::empty(full.num_nodes());
        let mut counterpart_strategy: Vec<f32> = Vec::new();
        for node in 0..full.num_nodes() {
            let PublicNode::Player {
                player,
                num_actions,
            } = full.node(node)
            else {
                continue;
            };
            let Counterpart {
                node: counterpart,
                relabelling,
            } = self.counterparts[node];
            let num_hands = full.num_hands(player);
            let len = num_actions * num_hands;
            let mut probabilities = vec![0.0; len];
            if self.members[relabelling as usize] == SuitPermutation::IDENTITY {
                // A node on a path of representatives plays its counterpart's strategy as it is.
                strategy(counterpart as usize, &mut probabilities);
            } else {
                if counterpart_strategy.len() < len {
                    counterpart_strategy.reserve_exact(len - counterpart_strategy.len());
                }
                counterpart_strategy.resize(len, 0.0);
                strategy(counterpart as usize, &mut counterpart_strategy);
                let relabelled = &self.relabelled[player][relabelling as usize];
                for (to, from) in probabilities
                    .chunks_exact_mut(num_hands)
                    .zip(counterpart_strategy.chunks_exact(num_hands))
                {
                    for (probability, &hand) in to.iter_mut().zip(relabelled) {
                        *probability = from[hand as usize];
                    }
                }
            }
            profile.set(node, num_hands, probabilities);
        }
        profile
    }

    /// The average strategy of `solver`, a solver of a merged game, on the full public tree: see
    /// [`MergedGame::expand`]. At a node on a path of representatives it is the solver's own,
    /// bit for bit.
    pub fn average_strategy(solver: &VectorCfr<MergedGame<'_>>) -> PublicProfile {
        let game = solver.game();
        game.expand(|node, out| {
            let sums = solver
                .strategy_sum(node)
                .expect("every merged player node has a row");
            pk_cfr::average_strategy(sums, out.len() / game.num_actions(node), out);
        })
    }

    /// A profile on the merged tree, such as a solver's current strategy, expanded onto the full
    /// public tree: see [`MergedGame::expand`].
    ///
    /// Panics if `profile` has no strategy, or one of the wrong size, at a merged player node.
    pub fn expand_profile(&self, profile: &PublicProfile) -> PublicProfile {
        self.expand(|node, out| {
            let strategy = profile
                .get(node)
                .unwrap_or_else(|| panic!("the profile has no strategy at merged node {node}"));
            out.copy_from_slice(strategy);
        })
    }

    fn num_actions(&self, node: usize) -> usize {
        match &self.nodes[node].kind {
            MergedKind::Player { children, .. } => children.len(),
            _ => panic!("node {node} is not a player node"),
        }
    }
}

impl PublicGame for MergedGame<'_> {
    fn num_hands(&self, player: usize) -> usize {
        self.full.num_hands(player)
    }

    fn deal(&self, player: usize) -> &[f64] {
        self.full.deal(player)
    }

    fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    fn root(&self) -> usize {
        0
    }

    fn node(&self, node: usize) -> PublicNode {
        match &self.nodes[node].kind {
            MergedKind::Player { actor, children } => PublicNode::Player {
                player: actor.index(),
                num_actions: children.len(),
            },
            MergedKind::Chance(chance) => PublicNode::Chance {
                num_outcomes: self.chances[*chance as usize].cards.len(),
            },
            MergedKind::Terminal => PublicNode::Terminal,
        }
    }

    fn child(&self, node: usize, index: usize) -> usize {
        MergedGame::child(self, node as u32, index) as usize
    }

    fn outcome_probability(&self, node: usize, _outcome: usize) -> f64 {
        self.chance(node).probability
    }

    fn rules_out(&self, node: usize, outcome: usize, player: usize, hand: usize) -> bool {
        let card = self.chance(node).cards[outcome];
        let combo = self.full.hands(player_of(player)).combos()[hand];
        combo.intersects(CardSet::single(card))
    }

    /// The hands that hold the representative, from the full game's lists.
    fn hands_ruled_out(
        &self,
        node: usize,
        outcome: usize,
        player: usize,
    ) -> impl Iterator<Item = usize> {
        let card = self.chance(node).cards[outcome];
        let holders = self.full.holders_of(player, card);
        holders.iter().map(|&hand| hand as usize)
    }

    /// Each image's relabelling of the player's hands: hand `h` after the image is hand `σ(h)`
    /// after the representative.
    fn images(&self, node: usize, outcome: usize, player: usize) -> impl Iterator<Item = &[u32]> {
        let chance = self.chance(node);
        let images =
            &chance.images[chance.starts[outcome] as usize..chance.starts[outcome + 1] as usize];
        let relabelled = &self.relabelled[player];
        images
            .iter()
            .map(move |image| &relabelled[image.relabelling as usize][..])
    }

    /// The hands that the node's stabilizer maps onto one another, each class named by its
    /// first hand: see [the module documentation](self#why-the-merged-solver-stays-symmetric).
    fn alike(&self, node: usize, player: usize) -> Option<&[u32]> {
        let alike = &self.stabilizers[self.nodes[node].stabilizer as usize].alike[player];
        (!alike.is_empty()).then_some(&alike[..])
    }

    /// Allocates the room [`PublicGame::terminal_values_with`] needs on a later board at each
    /// call.
    fn terminal_values(
        &self,
        node: usize,
        player: usize,
        opponent_reach: &[f64],
        values: &mut [f64],
    ) {
        self.terminal_values_with(node, player, opponent_reach, values, &mut Vec::new());
    }

    /// As the full game evaluates a terminal of the same betting node on the same board.
    fn terminal_values_with(
        &self,
        node: usize,
        player: usize,
        opponent_reach: &[f64],
        values: &mut [f64],
        scratch: &mut Vec<f64>,
    ) {
        let MergedNode { tree, board, .. } = self.nodes[node];
        self.full
            .terminal_values_on(tree, board, player, opponent_reach, values, scratch);
    }
}
