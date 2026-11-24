//! The pot, amounts to call and action order that follow from a config's forced bets.
//!
//! The rules are the same for every config:
//!
//! - The seat after the largest live blind acts first before the flop, and that blind's poster
//!   acts last.
//! - After the flop, the seat left of the button acts first and the button acts last.
//! - Antes go to the pot and never count toward a call.

use crate::config::{Chips, ForcedBetKind, GameConfig};
use crate::position::Position;

impl GameConfig {
    /// The positions at the table, clockwise from the seat left of the button.
    pub fn positions(&self) -> &'static [Position] {
        Position::table(self.players).expect("a loaded config has 2 to 9 players")
    }

    /// The largest live forced bet. Preflop raise sizes are multiples of it.
    pub fn effective_blind(&self) -> Chips {
        self.positions()
            .iter()
            .map(|&position| self.blind(position))
            .max()
            .unwrap_or(0)
    }

    /// All forced bets, live and dead.
    pub fn starting_pot(&self) -> Chips {
        self.forced_bets.iter().map(|bet| bet.amount).sum()
    }

    /// The dead money in the starting pot.
    pub fn dead_money(&self) -> Chips {
        let antes = self
            .forced_bets
            .iter()
            .filter(|bet| bet.kind == ForcedBetKind::Ante);
        antes.map(|bet| bet.amount).sum()
    }

    /// What a seat must add to call before anyone has raised.
    pub fn to_call(&self, position: Position) -> Chips {
        self.effective_blind() - self.blind(position)
    }

    /// The most a seat can bet in total, blinds included. It is the same for every seat.
    pub fn live_stack(&self) -> Chips {
        self.stack - self.ante(self.positions()[0])
    }

    /// The chips a seat has left after posting its forced bets.
    pub fn stack_behind(&self, position: Position) -> Chips {
        self.stack - self.ante(position) - self.blind(position)
    }

    /// The seat that closes the preflop action: the poster of the largest live blind. If two
    /// blinds are equal, the one further from the button in clockwise order closes.
    pub fn last_to_act_preflop(&self) -> Position {
        let seats = self.positions().iter().enumerate();
        let (_, &last) = seats
            .max_by_key(|&(seat, &position)| (self.blind(position), seat))
            .expect("a table has seats");
        last
    }

    /// The order of action before the flop.
    pub fn preflop_order(&self) -> Vec<Position> {
        let table = self.positions();
        let last = self.last_to_act_preflop();
        let last_seat = table
            .iter()
            .position(|&p| p == last)
            .expect("the closer has a seat");
        let first_seat = (last_seat + 1) % table.len();
        table[first_seat..]
            .iter()
            .chain(&table[..first_seat])
            .copied()
            .collect()
    }

    /// The order of action on the flop, turn and river.
    pub fn postflop_order(&self) -> Vec<Position> {
        self.positions().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CHIPS_PER_BB;
    use crate::position::Position::*;

    fn load(name: &str) -> GameConfig {
        let path = format!("{}/../../configs/{name}.toml", env!("CARGO_MANIFEST_DIR"));
        GameConfig::load(&path).unwrap_or_else(|err| panic!("{path} should load: {err}"))
    }

    /// A small config with the given seats and forced bets, for tables other than 8-handed.
    fn table(players: usize, forced_bets: &[(&str, f64, &str)]) -> GameConfig {
        let mut text = format!("name = \"T\"\nplayers = {players}\nstack_bb = 100.0\n");
        for (position, amount_bb, kind) in forced_bets {
            text += &format!(
                "\n[[forced_bets]]\nposition = \"{position}\"\namount_bb = {amount_bb:?}\nkind = \"{kind}\"\n"
            );
        }
        let config_c = include_str!("../../../configs/c.toml");
        text += &config_c[config_c.find("[rake]").unwrap()..];
        GameConfig::from_toml(&text).unwrap_or_else(|err| panic!("{text}\nshould load: {err}"))
    }

    const POSTFLOP_8: [Position; 8] = [Sb, Bb, Utg, Utg1, Lj, Hj, Co, Btn];

    #[test]
    fn configs_a_and_b_without_a_straddle() {
        for (name, stack_bb) in [("a", 100), ("b", 200)] {
            let config = load(name);
            assert_eq!(config.preflop_order(), [Utg, Utg1, Lj, Hj, Co, Btn, Sb, Bb]);
            assert_eq!(config.postflop_order(), POSTFLOP_8);
            assert_eq!(config.last_to_act_preflop(), Bb);

            assert_eq!(config.effective_blind(), 100);
            assert_eq!(config.starting_pot(), 150);
            assert_eq!(config.dead_money(), 0);
            assert_eq!(config.live_stack(), stack_bb * CHIPS_PER_BB);

            assert_eq!(config.to_call(Utg), 100);
            assert_eq!(config.to_call(Btn), 100);
            assert_eq!(config.to_call(Sb), 50);
            assert_eq!(config.to_call(Bb), 0);

            assert_eq!(config.stack_behind(Btn), stack_bb * CHIPS_PER_BB);
            assert_eq!(config.stack_behind(Sb), stack_bb * CHIPS_PER_BB - 50);
            assert_eq!(config.stack_behind(Bb), stack_bb * CHIPS_PER_BB - 100);
        }
    }

    #[test]
    fn configs_c_and_d_with_a_straddle_and_antes() {
        for (name, stack_bb) in [("c", 100), ("d", 200)] {
            let config = load(name);
            // The straddler acts last before the flop. After the flop nothing changes.
            assert_eq!(config.preflop_order(), [Utg1, Lj, Hj, Co, Btn, Sb, Bb, Utg]);
            assert_eq!(config.postflop_order(), POSTFLOP_8);
            assert_eq!(config.last_to_act_preflop(), Utg);

            // 0.5 + 1 + 2 from the blinds and straddle, and 8 x 0.5 from the antes.
            assert_eq!(config.effective_blind(), 200);
            assert_eq!(config.starting_pot(), 750);
            assert_eq!(config.dead_money(), 400);
            assert_eq!(config.live_stack(), stack_bb * CHIPS_PER_BB - 50);

            // Antes never count toward a call.
            assert_eq!(config.to_call(Utg1), 200);
            assert_eq!(config.to_call(Btn), 200);
            assert_eq!(config.to_call(Sb), 150);
            assert_eq!(config.to_call(Bb), 100);
            assert_eq!(config.to_call(Utg), 0);

            assert_eq!(config.stack_behind(Btn), stack_bb * CHIPS_PER_BB - 50);
            assert_eq!(config.stack_behind(Sb), stack_bb * CHIPS_PER_BB - 100);
            assert_eq!(config.stack_behind(Bb), stack_bb * CHIPS_PER_BB - 150);
            assert_eq!(config.stack_behind(Utg), stack_bb * CHIPS_PER_BB - 250);
        }
    }

    #[test]
    fn every_seat_acts_once_in_each_order() {
        for name in ["a", "b", "c", "d"] {
            let config = load(name);
            for order in [config.preflop_order(), config.postflop_order()] {
                assert_eq!(order.len(), config.players);
                for &position in config.positions() {
                    assert_eq!(order.iter().filter(|&&p| p == position).count(), 1);
                }
            }
            // Chips posted plus chips behind account for every stack.
            let behind: Chips = config
                .positions()
                .iter()
                .map(|&p| config.stack_behind(p))
                .sum();
            assert_eq!(
                behind + config.starting_pot(),
                config.stack * config.players as Chips
            );
        }
    }

    #[test]
    fn heads_up_the_button_posts_the_small_blind() {
        let config = table(2, &[("BTN", 0.5, "blind"), ("BB", 1.0, "blind")]);
        assert_eq!(config.preflop_order(), [Btn, Bb]);
        assert_eq!(config.postflop_order(), [Bb, Btn]);
        assert_eq!(config.to_call(Btn), 50);
        assert_eq!(config.starting_pot(), 150);
    }

    #[test]
    fn six_handed_with_and_without_a_straddle() {
        let plain = table(6, &[("SB", 0.5, "blind"), ("BB", 1.0, "blind")]);
        assert_eq!(plain.preflop_order(), [Utg, Hj, Co, Btn, Sb, Bb]);
        assert_eq!(plain.postflop_order(), [Sb, Bb, Utg, Hj, Co, Btn]);

        let straddled = table(
            6,
            &[
                ("SB", 0.5, "blind"),
                ("BB", 1.0, "blind"),
                ("UTG", 2.0, "blind"),
            ],
        );
        assert_eq!(straddled.preflop_order(), [Hj, Co, Btn, Sb, Bb, Utg]);
        assert_eq!(straddled.postflop_order(), [Sb, Bb, Utg, Hj, Co, Btn]);
        assert_eq!(straddled.effective_blind(), 200);
    }

    #[test]
    fn three_handed_the_button_acts_first_before_the_flop() {
        let config = table(
            3,
            &[
                ("SB", 0.5, "blind"),
                ("BB", 1.0, "blind"),
                ("ALL", 0.25, "ante"),
            ],
        );
        assert_eq!(config.preflop_order(), [Btn, Sb, Bb]);
        assert_eq!(config.postflop_order(), [Sb, Bb, Btn]);
        assert_eq!(config.starting_pot(), 225);
        assert_eq!(config.dead_money(), 75);
        assert_eq!(config.to_call(Btn), 100);
    }
}
