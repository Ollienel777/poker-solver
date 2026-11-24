//! Table positions.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

/// A seat at the table, named by its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Position {
    Sb,
    Bb,
    Utg,
    Utg1,
    Utg2,
    Lj,
    Hj,
    Co,
    Btn,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("`{0}` is not a position")]
pub struct ParsePositionError(String);

impl Position {
    pub const fn name(self) -> &'static str {
        match self {
            Position::Sb => "SB",
            Position::Bb => "BB",
            Position::Utg => "UTG",
            Position::Utg1 => "UTG1",
            Position::Utg2 => "UTG2",
            Position::Lj => "LJ",
            Position::Hj => "HJ",
            Position::Co => "CO",
            Position::Btn => "BTN",
        }
    }

    /// The positions at a table of `players` seats, clockwise from the seat left of the button.
    /// This is also the order of action after the flop.
    ///
    /// Heads-up, the button posts the small blind, so the two seats are BB and BTN.
    pub fn table(players: usize) -> Option<&'static [Position]> {
        use Position::*;
        Some(match players {
            2 => &[Bb, Btn],
            3 => &[Sb, Bb, Btn],
            4 => &[Sb, Bb, Co, Btn],
            5 => &[Sb, Bb, Hj, Co, Btn],
            6 => &[Sb, Bb, Utg, Hj, Co, Btn],
            7 => &[Sb, Bb, Utg, Lj, Hj, Co, Btn],
            8 => &[Sb, Bb, Utg, Utg1, Lj, Hj, Co, Btn],
            9 => &[Sb, Bb, Utg, Utg1, Utg2, Lj, Hj, Co, Btn],
            _ => return None,
        })
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Position {
    type Err = ParsePositionError;

    fn from_str(s: &str) -> Result<Position, ParsePositionError> {
        let all = Position::table(9).expect("a 9-handed table exists");
        all.iter()
            .copied()
            .find(|position| position.name().eq_ignore_ascii_case(s))
            .ok_or_else(|| ParsePositionError(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_position_round_trips() {
        for &position in Position::table(9).unwrap() {
            assert_eq!(position.to_string().parse::<Position>(), Ok(position));
            assert_eq!(
                position.name().to_lowercase().parse::<Position>(),
                Ok(position)
            );
        }
        assert!("MP".parse::<Position>().is_err());
        assert!("".parse::<Position>().is_err());
    }

    #[test]
    fn tables_have_one_seat_per_player_and_end_on_the_button() {
        for players in 2..=9 {
            let table = Position::table(players).unwrap();
            assert_eq!(table.len(), players);
            assert_eq!(table[players - 1], Position::Btn);
            assert!(table.contains(&Position::Bb));
            for (seat, position) in table.iter().enumerate() {
                assert!(
                    !table[..seat].contains(position),
                    "{position} repeats at {players} seats"
                );
            }
        }
        assert!(Position::table(1).is_none());
        assert!(Position::table(10).is_none());
    }

    #[test]
    fn eight_handed_names() {
        let names: Vec<&str> = Position::table(8)
            .unwrap()
            .iter()
            .map(|p| p.name())
            .collect();
        assert_eq!(names, ["SB", "BB", "UTG", "UTG1", "LJ", "HJ", "CO", "BTN"]);
    }
}
