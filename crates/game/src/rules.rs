//! Betting rules shared by the tree builders: whole-chip rounding, bet and raise sizes, the
//! minimum raise, the all-in conversions, the raise cap and duplicate removal.
//!
//! Nothing here assumes two players. A rule sees only the player to act, as a [`Stake`], and the
//! amounts on the table, so the N-player preflop builder can apply the same rules as the
//! heads-up postflop one. Every amount is in chips.
//!
//! Wagers are described by the street total they take the player to (a raise *to* 1,500),
//! not by the chips they add.

use std::cmp::Ordering;

use crate::config::Chips;

/// Sizes are nudged up by one part in 10^12 before they are rounded, so that a size that sits on
/// a boundary in decimal stays on it in binary floating point. 0.29 × 100, for example, is
/// 28.999999999999996 as an `f64`, and is counted as 29. The nudge is far larger than `f64`
/// error (about one part in 10^16 per operation) and far smaller than a chip, even at the
/// largest chip count.
const NUDGE: f64 = 1e-12;

/// Rounds an amount to whole chips: to the nearest chip, with halves rounded up. This is the one
/// function every bet and raise size goes through. Negative amounts and NaN give 0, and amounts
/// past `Chips::MAX` give `Chips::MAX`.
pub fn round_chips(chips: f64) -> Chips {
    // `as` saturates at both ends and sends NaN to 0.
    (chips * (1.0 + NUDGE) + 0.5).floor() as Chips
}

/// The most chips a player may have put in, in total, after a bet or raise that does not become
/// all-in: `threshold × stack`, rounded down. Because totals are whole chips, a total above this
/// limit is exactly a total of more than `threshold × stack`. With a threshold of 1 the limit is
/// the stack itself, which no total can pass, so nothing is converted.
pub fn commit_limit(stack: Chips, threshold: f64) -> Chips {
    (threshold * f64::from(stack) * (1.0 + NUDGE)).floor() as Chips
}

/// The smallest street total a raise may go to: the bet faced plus the last bet or raise
/// increment on the street. After an opening bet of 100, the minimum raise is to 200; after a
/// raise from 100 to 300, the next is to 500. It is always at least one chip more than the bet
/// faced, even given an increment of 0, so a raise always adds chips and a line of raises ends.
pub fn min_raise_to(faced: Chips, last_increment: Chips) -> Chips {
    faced.saturating_add(last_increment.max(1))
}

/// The increment that sets the next minimum raise, after a bet or raise to `to` facing `faced`.
///
/// It is the larger of `to - faced` and the previous increment. A full bet or raise is never
/// smaller than the previous increment, so this is the last increment. Only an all-in for less
/// than a full raise is smaller, and under the usual rule it leaves the minimum raise where it
/// was. (Heads-up that case never matters: the all-in player's opponent can only call or fold.)
pub fn next_increment(faced: Chips, to: Chips, last_increment: Chips) -> Chips {
    (to - faced).max(last_increment)
}

/// Whether a street that has had `raises` raises after its opening bet may have another. All-in
/// raises count.
pub fn under_raise_cap(raises: u32, max_raises: u32) -> bool {
    raises < max_raises
}

/// What the player to act has in play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stake {
    /// Chips put in on this street.
    pub street: Chips,
    /// Chips put in so far in all, `street` included. The all-in threshold counts these.
    pub total: Chips,
    /// Chips left to bet.
    pub behind: Chips,
}

impl Stake {
    /// The street total that going all-in takes this player to.
    pub fn allin_to(self) -> Chips {
        self.street + self.behind
    }
}

/// A bet or raise after the sizing rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wager {
    /// The player's total for the street after the wager.
    pub to: Chips,
    /// The configured size it came from: a fraction of the pot for a bet, a multiple of the bet
    /// faced for a raise. `None` means all-in.
    pub size: Option<f64>,
}

impl Wager {
    /// Going all-in.
    pub fn all_in(actor: Stake) -> Wager {
        Wager {
            to: actor.allin_to(),
            size: None,
        }
    }

    pub fn is_all_in(&self) -> bool {
        self.size.is_none()
    }
}

/// Applies the two all-in conversions to a bet or raise to `to` that came from `size`.
///
/// It becomes all-in if it would use all of the player's remaining chips or more
/// (`to >= actor.allin_to()`), or if it would leave the player's total above `limit`, the
/// [`commit_limit`].
pub fn wager(to: Chips, size: f64, actor: Stake, limit: Chips) -> Wager {
    let allin_to = actor.allin_to();
    // `to < allin_to` from here on, so the total cannot pass the stack.
    if to >= allin_to || actor.total - actor.street + to > limit {
        Wager::all_in(actor)
    } else {
        Wager {
            to,
            size: Some(size),
        }
    }
}

/// The bets a player can open the street with.
///
/// Each fraction `f` gives a bet of `f × pot`, rounded with [`round_chips`] and lifted to
/// `min_bet` if smaller, then passed through [`wager`]. With `allin`, going all-in is added. The
/// result is in increasing order of amount with duplicates removed, as [`dedup`] leaves it. It is
/// empty when the player has no chips behind.
///
/// A bet is at least one chip even if `min_bet` is 0, so that a bet always adds chips. Without
/// that, a pot of 0 would allow a line of 0-chip bets that never ends.
pub fn bets(
    pot: Chips,
    fractions: &[f64],
    allin: bool,
    min_bet: Chips,
    actor: Stake,
    limit: Chips,
) -> Vec<Wager> {
    if actor.behind == 0 {
        return Vec::new();
    }
    let mut wagers: Vec<Wager> = fractions
        .iter()
        .map(|&fraction| {
            let amount = round_chips(fraction * f64::from(pot)).max(min_bet).max(1);
            wager(actor.street.saturating_add(amount), fraction, actor, limit)
        })
        .collect();
    if allin {
        wagers.push(Wager::all_in(actor));
    }
    dedup(&mut wagers);
    wagers
}

/// The raises a player can make facing a street total of `faced`.
///
/// Each multiple `m` gives a raise to `m × faced`, rounded with [`round_chips`] and lifted to
/// [`min_raise_to`] if smaller, then passed through [`wager`]. With `allin`, going all-in is
/// added. A player who cannot make a full minimum raise can still raise all-in, since every size
/// then converts. The result is ordered and free of duplicates as for [`bets`]. It is empty when
/// the player cannot put in more than `faced`. The raise cap is the caller's to apply, with
/// [`under_raise_cap`].
pub fn raises(
    faced: Chips,
    last_increment: Chips,
    multiples: &[f64],
    allin: bool,
    actor: Stake,
    limit: Chips,
) -> Vec<Wager> {
    if actor.allin_to() <= faced {
        return Vec::new();
    }
    let min_to = min_raise_to(faced, last_increment);
    let mut wagers: Vec<Wager> = multiples
        .iter()
        .map(|&multiple| {
            let to = round_chips(multiple * f64::from(faced)).max(min_to);
            wager(to, multiple, actor, limit)
        })
        .collect();
    if allin {
        wagers.push(Wager::all_in(actor));
    }
    dedup(&mut wagers);
    wagers
}

/// Sorts wagers by amount and keeps one of each amount: the one from the smallest size. A sized
/// wager is always below the all-in amount, so all-in, if present, comes last and only once.
pub fn dedup(wagers: &mut Vec<Wager>) {
    wagers.sort_by(|a, b| a.to.cmp(&b.to).then_with(|| by_size(a.size, b.size)));
    wagers.dedup_by_key(|wager| wager.to);
}

/// Orders sizes from smallest to largest, with all-in last.
fn by_size(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.total_cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;

    use super::*;

    const DEEP: Stake = Stake {
        street: 0,
        total: 0,
        behind: 10_000,
    };

    fn sized(to: Chips, size: f64) -> Wager {
        Wager {
            to,
            size: Some(size),
        }
    }

    fn all_in(to: Chips) -> Wager {
        Wager { to, size: None }
    }

    #[test]
    fn rounding_goes_to_the_nearest_chip_with_halves_up() {
        assert_eq!(round_chips(49.4), 49);
        assert_eq!(round_chips(49.5), 50);
        assert_eq!(round_chips(49.6), 50);
        assert_eq!(round_chips(500.5), 501);
        assert_eq!(round_chips(0.0), 0);
        assert_eq!(round_chips(0.49), 0);
    }

    #[test]
    fn rounding_is_not_fooled_by_binary_floating_point() {
        // 0.29 x 100 is 28.999999999999996 as an f64, and 0.41 x 150 is 61.49999999999999.
        let (low_29, low_61_5) = (black_box(0.29) * 100.0, black_box(0.41) * 150.0);
        assert!(low_29 < 29.0 && low_61_5 < 61.5, "the premise of this test");
        assert_eq!(round_chips(low_29), 29);
        assert_eq!(round_chips(low_61_5), 62);
        // The nudge does not move amounts that are really below a half, even large ones.
        assert_eq!(round_chips(4_000_000_000.49), 4_000_000_000);
    }

    #[test]
    fn rounding_saturates_instead_of_wrapping() {
        assert_eq!(round_chips(1e300), Chips::MAX);
        assert_eq!(round_chips(f64::INFINITY), Chips::MAX);
        assert_eq!(round_chips(-5.0), 0);
        assert_eq!(round_chips(f64::NAN), 0);
    }

    #[test]
    fn the_commit_limit_is_the_threshold_of_the_stack_rounded_down() {
        assert_eq!(commit_limit(10_000, 0.67), 6_700);
        assert_eq!(commit_limit(1_001, 0.5), 500);
        // 0.29 x 100 is just under 29 in binary, but the limit is 29.
        assert_eq!(commit_limit(100, 0.29), 29);
        assert_eq!(commit_limit(9_750, 1.0), 9_750);
        assert_eq!(commit_limit(Chips::MAX, 1.0), Chips::MAX);
    }

    #[test]
    fn the_minimum_raise_adds_the_last_increment() {
        assert_eq!(min_raise_to(100, 100), 200);
        assert_eq!(min_raise_to(300, 200), 500);
        // A raise always adds at least a chip.
        assert_eq!(min_raise_to(100, 0), 101);
        assert_eq!(next_increment(0, 100, 0), 100);
        assert_eq!(next_increment(100, 300, 100), 200);
        // An all-in for less than a full raise leaves the increment where it was.
        assert_eq!(next_increment(300, 350, 200), 200);
    }

    #[test]
    fn the_raise_cap_counts_raises_after_the_bet() {
        assert!(under_raise_cap(0, 1));
        assert!(!under_raise_cap(1, 1));
        assert!(!under_raise_cap(0, 0));
    }

    #[test]
    fn bets_are_pot_fractions_of_at_least_one_big_blind() {
        // 0.33 x 1000 = 330; 0.75 x 1000 = 750.
        assert_eq!(
            bets(1_000, &[0.33, 0.75], false, 100, DEEP, 10_000),
            [sized(330, 0.33), sized(750, 0.75)]
        );
        // 0.25 x 200 = 50, lifted to the 100-chip minimum.
        assert_eq!(
            bets(200, &[0.25], false, 100, DEEP, 10_000),
            [sized(100, 0.25)]
        );
    }

    #[test]
    fn sizes_that_meet_or_pass_the_stack_become_all_in() {
        let short = Stake {
            street: 0,
            total: 0,
            behind: 700,
        };
        // 4 x 200 = 800 is more than the 700 left; 3.5 x 200 = 700 is exactly all of it.
        assert_eq!(bets(200, &[4.0], false, 100, short, 700), [all_in(700)]);
        assert_eq!(bets(200, &[3.5], false, 100, short, 700), [all_in(700)]);
        // With less than a big blind left, the minimum bet is all-in.
        let tiny = Stake {
            street: 0,
            total: 940,
            behind: 60,
        };
        assert_eq!(bets(100, &[0.1], false, 100, tiny, 1_000), [all_in(60)]);
    }

    #[test]
    fn sizes_past_the_commit_limit_become_all_in() {
        let actor = Stake {
            street: 0,
            total: 300,
            behind: 9_700,
        };
        // Limit 6,700 in all. A bet of 6,400 makes 6,700, which is not more than the limit; a
        // bet of 6,401 makes 6,701, which is.
        let limit = commit_limit(10_000, 0.67);
        assert_eq!(wager(6_400, 1.0, actor, limit), sized(6_400, 1.0));
        assert_eq!(wager(6_401, 1.0, actor, limit), all_in(9_700));
    }

    #[test]
    fn equal_amounts_keep_the_smallest_size_and_all_in_comes_last() {
        // At a pot of 150: 0.1 -> 15 and 0.33 -> 49.5 -> 50 are both lifted to 100, and 0.67 ->
        // 100.5 rounds up to 101. The order of the list does not matter.
        let wagers = bets(150, &[0.67, 0.33, 0.1], true, 100, DEEP, 10_000);
        assert_eq!(wagers, [sized(100, 0.1), sized(101, 0.67), all_in(10_000)]);

        // A size converted to all-in and the all-in flag are one action.
        let wagers = bets(1_000, &[0.5, 20.0], true, 100, DEEP, 10_000);
        assert_eq!(wagers, [sized(500, 0.5), all_in(10_000)]);
    }

    #[test]
    fn a_bet_adds_a_chip_even_with_no_minimum() {
        // At a pot of 0 every fraction gives 0 chips. With no minimum a bet is still one chip,
        // so a line of bets cannot go on forever.
        assert_eq!(bets(0, &[0.5], false, 0, DEEP, 10_000), [sized(1, 0.5)]);
    }

    #[test]
    fn no_bets_without_chips_behind() {
        let broke = Stake {
            street: 0,
            total: 10_000,
            behind: 0,
        };
        assert!(bets(1_000, &[0.5], true, 100, broke, 10_000).is_empty());
    }

    #[test]
    fn raises_are_multiples_of_the_bet_faced_lifted_to_the_minimum() {
        // Facing a bet of 100: 1.2 -> 120 and 1.5 -> 150 are lifted to the minimum raise to 200;
        // 2.5 -> 250 stands.
        assert_eq!(
            raises(100, 100, &[2.5, 1.5, 1.2], false, DEEP, 10_000),
            [sized(200, 1.2), sized(250, 2.5)]
        );
        // Facing a raise from 100 to 300, the minimum is to 500, so 1.5 x 300 = 450 is lifted.
        let raiser = Stake {
            street: 100,
            total: 100,
            behind: 9_900,
        };
        assert_eq!(
            raises(300, 200, &[1.5, 3.0], false, raiser, 10_000),
            [sized(500, 1.5), sized(900, 3.0)]
        );
    }

    #[test]
    fn a_player_short_of_a_minimum_raise_can_still_raise_all_in() {
        // The minimum raise is to 200 but only 150 can go in: every size becomes all-in, and so
        // does the flag.
        let short = Stake {
            street: 0,
            total: 0,
            behind: 150,
        };
        assert_eq!(raises(100, 100, &[3.0], false, short, 150), [all_in(150)]);
        assert_eq!(raises(100, 100, &[], true, short, 150), [all_in(150)]);
        // Without sizes or the flag there is nothing to raise with.
        assert!(raises(100, 100, &[], false, short, 150).is_empty());
    }

    #[test]
    fn no_raises_when_a_call_uses_every_chip() {
        let actor = Stake {
            street: 0,
            total: 0,
            behind: 150,
        };
        assert!(raises(150, 150, &[3.0], true, actor, 150).is_empty());
        assert!(raises(200, 200, &[3.0], true, actor, 150).is_empty());
    }
}
