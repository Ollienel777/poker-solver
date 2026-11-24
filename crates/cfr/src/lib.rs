//! Regret matching, discount schedules, toy-game CFR and best response.
//!
//! This crate knows nothing about poker. It is built in milestone M2.

pub mod best_response;
pub mod kuhn;
pub mod leduc;
pub mod regret;
pub mod tabular;

pub use best_response::{BestResponse, best_response, exploitability};
pub use kuhn::{Kuhn, KuhnCard, KuhnInfoSet, KuhnState};
pub use leduc::{Leduc, LeducAction, LeducCard, LeducInfoSet, LeducRank, LeducState};
pub use regret::{
    Dcfr, Discount, Schedule, accumulate_strategy, average_strategy, current_strategy,
    update_regrets,
};
pub use tabular::{Cfr, Game, Node, Profile, expected_values};
