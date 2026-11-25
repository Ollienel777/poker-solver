//! How the commands write amounts, sizes and times, and how they print the spot.
//!
//! Amounts are in big blinds, as a player reads them: exactly for the pot, the stack and the
//! rake cap, which are whole chips, and to two decimals for values, which are averages. Bet
//! sizes are percents of the pot and raise sizes multiples of the bet, as in the config. Memory
//! is in decimal units, as the library's estimate writes it. Each report line is a label padded
//! to [`LABEL`] characters and its value, so that the values line up.

use std::fmt;
use std::time::Duration;

use pk_game::{CHIPS_PER_BB, Player, StreetSizing};
use pk_postflop::MemoryEstimate;
use pk_postflop::memory::format_bytes;

use crate::spot_input::{Source, SpotInput};

/// The width of a report line's label.
pub const LABEL: usize = 13;

/// Prints one report line: the label, padded to [`LABEL`], and the value.
pub fn field(label: &str, value: impl fmt::Display) {
    println!("{label:<LABEL$}{value}");
}

/// An amount of whole chips in big blinds, exactly and without trailing zeros: `5.5bb`, `97.25bb`,
/// `6bb`.
pub fn bb(chips: u32) -> String {
    let per_bb = CHIPS_PER_BB;
    let (whole, cents) = (chips / per_bb, chips % per_bb);
    match cents {
        0 => format!("{whole}bb"),
        tenths if tenths % 10 == 0 => format!("{whole}.{}bb", tenths / 10),
        cents => format!("{whole}.{cents:02}bb"),
    }
}

/// An amount of chips that need not be whole, in big blinds to `decimals` places: `2.31bb`.
pub fn bb_value(chips: f64, decimals: usize) -> String {
    format!("{:.decimals$}bb", chips / f64::from(CHIPS_PER_BB))
}

/// A percent of the pot, to four decimals: `0.2789%`.
pub fn percent(value: f64) -> String {
    format!("{value:.4}%")
}

/// A count with a comma between each three digits: `191,844`.
pub fn thousands(count: u64) -> String {
    let digits = count.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// Bytes in decimal units, as the library's memory estimate writes them: `1.10 GB`, `613.0 MB`.
pub fn bytes(bytes: u64) -> String {
    format_bytes(bytes)
}

/// A duration: `0.004s` and `2.350s` under ten seconds, `21.30s` under a minute, `3m 05.2s`
/// under an hour, then `1h 02m 05s`.
pub fn duration(time: Duration) -> String {
    let seconds = time.as_secs_f64();
    if seconds < 10.0 {
        format!("{seconds:.3}s")
    } else if seconds < 60.0 {
        format!("{seconds:.2}s")
    } else if seconds < 3600.0 {
        let minutes = (seconds / 60.0).floor();
        format!("{minutes}m {:04.1}s", seconds - 60.0 * minutes)
    } else {
        let whole = time.as_secs();
        format!(
            "{}h {:02}m {:02}s",
            whole / 3600,
            (whole % 3600) / 60,
            whole % 60
        )
    }
}

/// `1 thread` or `16 threads`.
pub fn threads(count: u64) -> String {
    match count {
        1 => "1 thread".to_string(),
        count => format!("{count} threads"),
    }
}

/// A number without trailing zeros, to at most two decimals: `33`, `2.5`, `33.33`.
fn number(value: f64) -> String {
    // Rounding to two decimals keeps `0.07 × 100` from printing as 7.000000000000001.
    let rounded = (value * 100.0).round() / 100.0;
    format!("{}", rounded + 0.0)
}

/// `a`, `a and b`, or `a, b and c`.
pub fn list(items: &[String]) -> String {
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => items.concat(),
    }
}

/// One street's sizing: `bets 33% and 75% of the pot; raises to 3x the bet; all-in`. Raise sizes
/// are left out when the tree allows no raise.
pub fn street_sizing(sizing: &StreetSizing, max_raises: u32) -> String {
    let bets: Vec<String> = sizing
        .bets
        .iter()
        .map(|&bet| format!("{}%", number(100.0 * bet)))
        .collect();
    let mut parts = vec![format!("bets {} of the pot", list(&bets))];
    if max_raises > 0 && !sizing.raises.is_empty() {
        let raises: Vec<String> = sizing
            .raises
            .iter()
            .map(|&raise| format!("{}x", number(raise)))
            .collect();
        parts.push(format!("raises to {} the bet", list(&raises)));
    }
    if sizing.allin {
        parts.push("all-in".to_string());
    }
    parts.join("; ")
}

/// The two rules every street follows: `at most 3 raises a street; a bet or raise that would put
/// in more than 67% of the stack goes all-in`.
pub fn tree_rules(max_raises: u32, allin_threshold: f64) -> String {
    let raises = match max_raises {
        0 => "no raises".to_string(),
        1 => "at most 1 raise a street".to_string(),
        count => format!("at most {count} raises a street"),
    };
    if allin_threshold < 1.0 {
        format!(
            "{raises}; a bet or raise that would put in more than {}% of the stack goes all-in",
            number(100.0 * allin_threshold)
        )
    } else {
        raises
    }
}

/// Prints the spot: its name, where it came from, the board, the players, the pot and stack,
/// the rake and the tree, a line each and the tree a line per street.
pub fn print_spot(input: &SpotInput) {
    let spot = &input.spot;
    field("Spot", &spot.name);
    field("From", &input.source);
    field(
        "Board",
        format!("{}, starting on the {}", spot.board, spot.street()),
    );
    if let Source::RangeFile { positions, .. } = &input.source {
        field(
            "Players",
            format!(
                "{} out of position, {} in position",
                positions[0], positions[1]
            ),
        );
    }
    field(
        "Pot",
        format!(
            "{}, with {} behind each player",
            bb(spot.pot),
            bb(spot.effective_stack)
        ),
    );
    let rake = if spot.rake_percent == 0.0 {
        "none".to_string()
    } else {
        format!(
            "{}% of the pot, capped at {}",
            spot.rake_percent,
            bb(spot.rake_cap)
        )
    };
    field("Rake", rake);
    field("Tree", &input.tree);
    let tree = &spot.tree;
    for street in pk_game::Street::ALL {
        if let Some(sizing) = tree.street(street) {
            field(
                &format!("  {street}"),
                street_sizing(sizing, tree.max_raises),
            );
        }
    }
    field("  rules", tree_rules(tree.max_raises, tree.allin_threshold));
}

/// Prints each player's hands and the size of the trees, from the memory estimate.
pub fn print_sizes(input: &SpotInput, estimate: &MemoryEstimate) {
    field(
        "Hands",
        format!(
            "{} {}, {} {}, of the combos the board leaves",
            input.player(Player::Oop),
            thousands(estimate.hands[0]),
            input.player(Player::Ip),
            thousands(estimate.hands[1])
        ),
    );
    let mut nodes = format!(
        "{} in the public tree, from {} in the betting tree",
        thousands(estimate.public_nodes),
        thousands(estimate.betting_nodes)
    );
    if estimate.merged {
        nodes.push_str(&format!(
            "; {} once merged by suit symmetry",
            thousands(estimate.merged_nodes)
        ));
    }
    field("Nodes", nodes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_are_in_big_blinds() {
        assert_eq!(bb(550), "5.5bb");
        assert_eq!(bb(9725), "97.25bb");
        assert_eq!(bb(600), "6bb");
        assert_eq!(bb(1), "0.01bb");
        assert_eq!(bb(0), "0bb");
        assert_eq!(bb_value(231.4, 2), "2.31bb");
        assert_eq!(bb_value(-1.5, 4), "-0.0150bb");
    }

    #[test]
    fn counts_have_a_comma_every_three_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(191_844), "191,844");
        assert_eq!(thousands(9_789_026), "9,789,026");
    }

    #[test]
    fn durations_read_in_seconds_then_minutes_then_hours() {
        assert_eq!(duration(Duration::from_micros(4200)), "0.004s");
        assert_eq!(duration(Duration::from_millis(2350)), "2.350s");
        assert_eq!(duration(Duration::from_secs_f64(21.304)), "21.30s");
        assert_eq!(duration(Duration::from_secs_f64(185.24)), "3m 05.2s");
        assert_eq!(duration(Duration::from_secs(3725)), "1h 02m 05s");
    }

    #[test]
    fn sizes_are_percents_of_the_pot_and_multiples_of_the_bet() {
        let sizing = StreetSizing {
            bets: vec![0.33, 0.75],
            raises: vec![2.5, 3.0],
            allin: true,
        };
        assert_eq!(
            street_sizing(&sizing, 3),
            "bets 33% and 75% of the pot; raises to 2.5x and 3x the bet; all-in"
        );
        assert_eq!(
            street_sizing(&sizing, 0),
            "bets 33% and 75% of the pot; all-in"
        );
        let one = StreetSizing {
            bets: vec![0.07],
            raises: vec![],
            allin: false,
        };
        assert_eq!(street_sizing(&one, 1), "bets 7% of the pot");
        assert_eq!(
            tree_rules(3, 0.67),
            "at most 3 raises a street; a bet or raise that would put in more than 67% of the \
             stack goes all-in"
        );
        assert_eq!(tree_rules(0, 1.0), "no raises");
        assert_eq!(tree_rules(1, 1.0), "at most 1 raise a street");
    }

    #[test]
    fn lists_join_with_commas_and_a_last_and() {
        let items = |items: &[&str]| {
            items
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(list(&items(&[])), "");
        assert_eq!(list(&items(&["a"])), "a");
        assert_eq!(list(&items(&["a", "b"])), "a and b");
        assert_eq!(list(&items(&["a", "b", "c"])), "a, b and c");
    }
}
