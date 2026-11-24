//! Regret matching, discount schedules, toy-game CFR and best response.
//!
//! This crate knows nothing about poker. It is built in milestone M2.

pub mod regret;

pub use regret::{
    Dcfr, Discount, Schedule, accumulate_strategy, average_strategy, current_strategy,
    update_regrets,
};
