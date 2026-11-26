//! The commands, one module each. A command's module holds its arguments, as a struct that
//! derives [`clap::Args`], and its `run`, which prints what the command reports and returns an
//! error for `main` to print.

pub mod compare;
pub mod estimate;
pub mod inspect;
pub mod serve;
pub mod solve_postflop;
