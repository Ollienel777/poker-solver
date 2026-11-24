//! What each player wins or loses at a terminal node, rake included.
//!
//! # Baseline
//!
//! A payoff is the net change in a player's chips from the root of the tree to the end of the
//! hand. Chips the players put in before the root are already in the root pot `P` and count
//! for neither player, so over both players the payoffs at a terminal always sum to `P` minus
//! the rake. `contributions` are the chips each player put in during the tree, indexed by
//! [`Player::index`], as a [`Node::Terminal`](pk_game::Node::Terminal) holds them.
//!
//! # Rake
//!
//! The rake on a pot of `pot` chips is `min(cap, percent / 100 × pot)` ([`rake_on`]), in `f64`
//! with no rounding to whole chips. It is zero when the percent is zero. Every postflop pot is
//! raked: no flop no drop applies only to hands that end before the flop, so
//! [`Rake::no_flop_no_drop`] is ignored here.
//!
//! # Showdown
//!
//! A showdown has equal contributions `c`, so the final pot is `P + 2c`, raked as a whole:
//!
//! | Result | Payoff |
//! |---|---|
//! | win | `P + c − rake` |
//! | tie | `(P − rake) / 2` |
//! | lose | `−c` |
//!
//! # Fold
//!
//! The player who folds has put in `c[loser]`, and the winner at least that much. The part of
//! the winner's last bet that was not called goes back to the winner and is not raked, so the
//! pot that counts is `P + 2 · c[loser]`:
//!
//! | Player | Payoff |
//! |---|---|
//! | winner | `P + c[loser] − rake` |
//! | loser, who folded | `−c[loser]` |

use pk_game::{Chips, Outcome, Player, Rake};

/// The rake taken from a pot of `pot` chips: `min(cap, percent / 100 × pot)`, in `f64` with no
/// rounding to whole chips, and zero when the percent is zero.
///
/// It is computed as `percent × pot / 100`, so that a whole percent of a whole pot is rounded
/// once: 5% of 1,555 chips is exactly 77.75.
///
/// Panics unless the percent is from 0 to 100.
pub fn rake_on(pot: f64, rake: &Rake) -> f64 {
    assert!(
        (0.0..=100.0).contains(&rake.percent),
        "rake percent {} is outside 0 to 100",
        rake.percent
    );
    if rake.percent == 0.0 {
        return 0.0;
    }
    (rake.percent * pot / 100.0).min(f64::from(rake.cap))
}

/// One player's payoff at a showdown for each result against the other player's hand. Both
/// players have the same three numbers, because a showdown's contributions are equal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShowdownPayoff {
    /// The payoff for holding the stronger hand.
    pub win: f64,
    /// The payoff for a split pot.
    pub tie: f64,
    /// The payoff for holding the weaker hand.
    pub lose: f64,
}

/// The payoffs at a showdown with root pot `root_pot` and the given contributions, as the
/// [module documentation](self) defines them.
///
/// Panics if the contributions differ: a showdown is only reached once every bet is called.
pub fn showdown_payoff(root_pot: Chips, contributions: [Chips; 2], rake: &Rake) -> ShowdownPayoff {
    let [oop, ip] = contributions;
    assert_eq!(oop, ip, "a showdown needs equal contributions");
    let (pot, put_in) = (f64::from(root_pot), f64::from(oop));
    let rake = rake_on(pot + 2.0 * put_in, rake);
    ShowdownPayoff {
        win: pot + put_in - rake,
        tie: (pot - rake) / 2.0,
        lose: -put_in,
    }
}

/// Each player's payoff, indexed by [`Player::index`], when the opponent of `winner` folds, as
/// the [module documentation](self) defines them. The payoffs do not depend on the cards.
///
/// Panics if the winner put in less than the player who folded: a player only folds facing a
/// bet.
pub fn fold_payoffs(
    root_pot: Chips,
    winner: Player,
    contributions: [Chips; 2],
    rake: &Rake,
) -> [f64; 2] {
    let loser = winner.opponent();
    let (won, lost) = (contributions[winner.index()], contributions[loser.index()]);
    assert!(
        won >= lost,
        "the winner put in {won} chips, less than the {lost} of the player who folded"
    );
    let (pot, called) = (f64::from(root_pot), f64::from(lost));
    let rake = rake_on(pot + 2.0 * called, rake);
    let mut payoffs = [0.0; 2];
    payoffs[winner.index()] = pot + called - rake;
    payoffs[loser.index()] = -called;
    payoffs
}

/// The payoffs at one terminal node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Payoff {
    /// After a fold: each player's payoff, indexed by [`Player::index`], whatever the cards.
    Fold([f64; 2]),
    /// At a showdown: either player's payoff by result.
    Showdown(ShowdownPayoff),
}

impl Payoff {
    /// The payoffs at a terminal node with this outcome and these contributions, in a tree
    /// whose root pot is `root_pot`. See [`fold_payoffs`] and [`showdown_payoff`].
    pub fn new(
        root_pot: Chips,
        outcome: Outcome,
        contributions: [Chips; 2],
        rake: &Rake,
    ) -> Payoff {
        match outcome {
            Outcome::Fold { winner } => {
                Payoff::Fold(fold_payoffs(root_pot, winner, contributions, rake))
            }
            Outcome::Showdown => Payoff::Showdown(showdown_payoff(root_pot, contributions, rake)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5% capped at 3bb, as in the raked river test spots.
    fn five_percent() -> Rake {
        Rake {
            percent: 5.0,
            cap: 300,
            no_flop_no_drop: true,
        }
    }

    fn no_rake() -> Rake {
        Rake {
            percent: 0.0,
            cap: 300,
            no_flop_no_drop: true,
        }
    }

    fn showdown(win: f64, tie: f64, lose: f64) -> ShowdownPayoff {
        ShowdownPayoff { win, tie, lose }
    }

    #[test]
    fn rake_is_the_percent_of_the_pot_up_to_the_cap() {
        let rake = five_percent();
        assert_eq!(rake_on(0.0, &rake), 0.0);
        assert_eq!(rake_on(2000.0, &rake), 100.0);
        // Not rounded to whole chips.
        assert_eq!(rake_on(555.0, &rake), 27.75);
        // 5% of 6,000 is exactly the cap, and anything above it is capped.
        assert_eq!(rake_on(6000.0, &rake), 300.0);
        assert_eq!(rake_on(11_000.0, &rake), 300.0);
        assert_eq!(rake_on(1e9, &rake), 300.0);
    }

    #[test]
    fn zero_percent_takes_no_rake_and_a_zero_cap_takes_none_either() {
        assert_eq!(rake_on(1e9, &no_rake()), 0.0);
        let uncapped_zero = Rake {
            cap: Chips::MAX,
            ..no_rake()
        };
        assert_eq!(rake_on(1e9, &uncapped_zero), 0.0);
        let zero_cap = Rake {
            cap: 0,
            ..five_percent()
        };
        assert_eq!(rake_on(2000.0, &zero_cap), 0.0);
    }

    #[test]
    fn no_flop_no_drop_does_not_matter_after_the_flop() {
        let always = Rake {
            no_flop_no_drop: false,
            ..five_percent()
        };
        assert_eq!(rake_on(2000.0, &always), rake_on(2000.0, &five_percent()));
        assert_eq!(
            showdown_payoff(1000, [500, 500], &always),
            showdown_payoff(1000, [500, 500], &five_percent())
        );
    }

    #[test]
    #[should_panic(expected = "outside 0 to 100")]
    fn a_rake_above_100_percent_is_rejected() {
        let rake = Rake {
            percent: 150.0,
            ..five_percent()
        };
        rake_on(100.0, &rake);
    }

    #[test]
    fn showdown_without_rake_pays_the_pot() {
        // A 10bb pot, then a 5bb bet and a call: the final pot is 20bb.
        assert_eq!(
            showdown_payoff(1000, [500, 500], &no_rake()),
            showdown(1500.0, 500.0, -500.0)
        );
        // Checked down: the winner gets the root pot and the loser loses nothing.
        assert_eq!(
            showdown_payoff(1000, [0, 0], &no_rake()),
            showdown(1000.0, 500.0, 0.0)
        );
    }

    #[test]
    fn showdown_rake_below_the_cap_comes_off_the_whole_pot() {
        // The final pot is 2,000 chips, and 5% of it is 100, below the 300 cap.
        assert_eq!(
            showdown_payoff(1000, [500, 500], &five_percent()),
            showdown(1400.0, 450.0, -500.0)
        );
        // An odd pot: 5% of 1,555 is 77.75, split in a tie.
        assert_eq!(
            showdown_payoff(555, [500, 500], &five_percent()),
            showdown(977.25, 238.625, -500.0)
        );
    }

    #[test]
    fn showdown_rake_is_capped() {
        // The final pot is 11,000 chips. 5% would be 550, so the 300 cap applies.
        assert_eq!(
            showdown_payoff(1000, [5000, 5000], &five_percent()),
            showdown(5700.0, 350.0, -5000.0)
        );
    }

    #[test]
    #[should_panic(expected = "equal contributions")]
    fn a_showdown_with_unequal_contributions_is_rejected() {
        showdown_payoff(1000, [500, 700], &no_rake());
    }

    #[test]
    fn the_uncalled_part_of_a_bet_goes_back_unraked() {
        // OOP bets 200, IP raises to 900 and OOP folds. Only 200 of IP's raise was called, so
        // the pot that counts is 1,000 + 2 × 200 = 1,400, raked 70.
        let payoffs = fold_payoffs(1000, Player::Ip, [200, 900], &five_percent());
        assert_eq!(payoffs, [-200.0, 1130.0]);
        // Without rake, IP wins the root pot and OOP's 200.
        let payoffs = fold_payoffs(1000, Player::Ip, [200, 900], &no_rake());
        assert_eq!(payoffs, [-200.0, 1200.0]);
    }

    #[test]
    fn a_fold_to_the_first_bet_wins_the_root_pot_less_its_rake() {
        // OOP checks, IP bets 500 and OOP folds: OOP put in nothing.
        let payoffs = fold_payoffs(1000, Player::Ip, [0, 500], &five_percent());
        assert_eq!(payoffs, [0.0, 950.0]);
        // OOP bets into IP, who folds, on a later street after both put in 2,000.
        let payoffs = fold_payoffs(1000, Player::Oop, [9000, 2000], &five_percent());
        // The pot that counts is 5,000; 5% is 250, below the cap.
        assert_eq!(payoffs, [2750.0, -2000.0]);
        // A pot of 11,000 that counts is capped at 300.
        let payoffs = fold_payoffs(1000, Player::Oop, [9000, 5000], &five_percent());
        assert_eq!(payoffs, [5700.0, -5000.0]);
    }

    #[test]
    #[should_panic(expected = "less than")]
    fn a_fold_by_the_player_who_put_in_more_is_rejected() {
        fold_payoffs(1000, Player::Oop, [200, 900], &no_rake());
    }

    #[test]
    fn payoffs_sum_to_the_root_pot_less_the_rake() {
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs().max(1.0);
        for rake in [no_rake(), five_percent()] {
            for (pot, c) in [(0, 0), (150, 0), (1000, 500), (555, 12_345), (999, 7)] {
                let s = showdown_payoff(pot, [c, c], &rake);
                let raked = f64::from(pot) - rake_on(f64::from(pot + 2 * c), &rake);
                assert!(close(s.win + s.lose, raked), "{s:?}");
                assert_eq!(2.0 * s.tie, raked);
                for winner in [Player::Oop, Player::Ip] {
                    let mut contributions = [c + 300; 2];
                    contributions[winner.opponent().index()] = c;
                    let [oop, ip] = fold_payoffs(pot, winner, contributions, &rake);
                    assert!(close(oop + ip, raked), "{oop} + {ip}");
                }
            }
        }
    }

    #[test]
    fn payoff_follows_the_terminal_outcome() {
        let rake = five_percent();
        assert_eq!(
            Payoff::new(1000, Outcome::Showdown, [500, 500], &rake),
            Payoff::Showdown(showdown_payoff(1000, [500, 500], &rake))
        );
        let fold = Outcome::Fold {
            winner: Player::Oop,
        };
        assert_eq!(
            Payoff::new(1000, fold, [900, 200], &rake),
            Payoff::Fold([1130.0, -200.0])
        );
    }
}
