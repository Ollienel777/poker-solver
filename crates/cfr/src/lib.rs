//! Regret matching, discount schedules, toy-game CFR and best response.
//!
//! This crate knows nothing about poker. It is built in milestone M2.

pub mod best_response;
pub mod compressed;
pub mod kuhn;
pub mod leduc;
pub mod mccfr;
pub mod public_leduc;
pub mod regret;
pub mod tabular;
pub mod vector;
pub mod vector_best_response;

pub use best_response::{BestResponse, best_response, exploitability};
pub use compressed::CompressedStorage;
pub use kuhn::{Kuhn, KuhnCard, KuhnInfoSet, KuhnState};
pub use leduc::{Leduc, LeducAction, LeducCard, LeducInfoSet, LeducRank, LeducState};
pub use mccfr::Mccfr;
pub use public_leduc::PublicLeduc;
pub use regret::{
    Dcfr, Discount, Schedule, accumulate_strategy, average_strategy, current_strategy,
    update_regrets,
};
pub use tabular::{Cfr, Game, Node, Profile, expected_values};
pub use vector::{F32Storage, PublicGame, PublicNode, PublicProfile, Storage, VectorCfr};
pub use vector_best_response::{
    VectorBestResponse, VectorBestResponseValue, VectorExpectedValue, vector_best_response,
    vector_best_response_value, vector_expected_value, vector_expected_values,
    vector_exploitability,
};
