//! Range files: the two ranges of a heads-up spot at the flop, with its pot and stacks.
//!
//! A range file is TOML. It is designed to be typed by hand from a reference solver's range
//! view, and `export-ranges` (milestone M8) writes the same format, so one reader serves both:
//!
//! ```toml
//! format = 1
//! config = "a"
//! formation = "BTN_vs_BB_SRP"
//! line = "UTG f, UTG1 f, LJ f, HJ f, CO f, BTN r2.5, SB f, BB c"
//! pot_bb = 5.5
//! effective_stack_bb = 97.5
//! source = "illustrative, not from a reference"
//!
//! [oop]
//! position = "BB"
//! range = "22-JJ,A2s-AJs,K2s+,QQ:0.5"
//!
//! [ip]
//! position = "BTN"
//! range = "22+,A2s+,K5s+,A2o-A6o:0.5"
//! ```
//!
//! # Fields
//!
//! Every key is required, and no other key is allowed.
//!
//! - `format` is the integer 1.
//! - `config` names the game config the spot belongs to, such as `a` for config A.
//! - `formation` is the formation name, such as `BTN_vs_BB_SRP`, and `line` the preflop actions
//!   that reach the flop. Both are free text for now, but neither may be blank.
//! - `pot_bb` is the pot at the flop in big blinds, antes and folded blinds included.
//! - `effective_stack_bb` is what each of the two players has left to bet at the flop.
//! - `source` says where the ranges came from. It may not be blank.
//! - `[oop]` is the player who acts first after the flop and `[ip]` the one who acts last. Each
//!   has a `position`, a name such as `BB` in any case, and a `range` in the range-string syntax
//!   of [`pk_core::range`]. A range must hold at least one hand, and the two positions must
//!   differ.
//!
//! The two amounts may be written as integers or decimals. Each must be a positive whole number
//! of chips (1bb = 100 chips), at most 1,000,000bb.
//!
//! # Reading, checking and writing
//!
//! [`RangeFile::from_toml`] and [`RangeFile::load`] check everything above. Each error names the
//! field, such as `oop.range` or `pot_bb`, and says what is wrong. A range string that does not
//! parse passes on the range parser's own message. A misspelled key is reported as an unknown
//! key rather than as the missing one it was meant to be.
//!
//! [`RangeFile::check`] then checks the file against its [`GameConfig`]: the config's name, the
//! positions at its table, which side acts first after the flop, and whether the pot and stack
//! can be reached under its forced bets.
//!
//! [`RangeFile::to_toml`] writes the layout above: the keys in that order, each range in its
//! canonical form (`Display` for [`Range`]), positions in capitals and amounts as decimals. A
//! hand-typed file and an exported one therefore look alike, and reading what was written gives
//! back an equal file.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use pk_core::Range;
use thiserror::Error;
use toml::{Table, Value};

use crate::config::{CHIPS_PER_BB, Chips, GameConfig, Invalid, MAX_BB, to_chips};
use crate::position::Position;
use crate::tree::Player;

/// The version of the range-file format that this module reads and writes.
pub const RANGE_FILE_FORMAT: i64 = 1;

/// The keys of a range file, in the order they are written.
const KEYS: [&str; 9] = [
    "format",
    "config",
    "formation",
    "line",
    "pot_bb",
    "effective_stack_bb",
    "source",
    "oop",
    "ip",
];

/// The keys of `[oop]` and `[ip]`.
const PLAYER_KEYS: [&str; 2] = ["position", "range"];

/// The largest amount a range file may hold, the same as a config's.
const MAX_CHIPS: Chips = MAX_BB as Chips * CHIPS_PER_BB;

// Amounts are written with two decimals at most.
const _: () = assert!(CHIPS_PER_BB == 100);

#[derive(Debug, Error)]
pub enum RangeFileError {
    #[error("could not read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The text is not valid TOML.
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    /// A field is missing, unknown, of the wrong type, or holds a value that is not allowed.
    #[error("`{field}` {reason}")]
    Invalid { field: String, reason: String },
}

impl From<Invalid> for RangeFileError {
    fn from(invalid: Invalid) -> RangeFileError {
        RangeFileError::Invalid {
            field: invalid.field,
            reason: invalid.reason,
        }
    }
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> RangeFileError {
    Invalid::new(field, reason).into()
}

/// One player of a range file: a seat and its range at the flop.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRange {
    pub position: Position,
    pub range: Range,
}

/// A heads-up formation at the flop: who is in the pot, with which ranges, and how much is in
/// play. The [module documentation](self) describes the file.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeFile {
    /// The name of the game config, such as `a`.
    pub config: String,
    /// The formation, such as `BTN_vs_BB_SRP`. Free text for now.
    pub formation: String,
    /// The preflop actions that reach the flop. Free text for now.
    pub line: String,
    /// The pot at the flop, antes and folded blinds included.
    pub pot: Chips,
    /// What each player has left to bet at the flop.
    pub effective_stack: Chips,
    /// Where the ranges came from.
    pub source: String,
    /// The player who acts first after the flop.
    pub oop: PlayerRange,
    /// The player who acts last after the flop.
    pub ip: PlayerRange,
}

impl RangeFile {
    pub fn load(path: impl AsRef<Path>) -> Result<RangeFile, RangeFileError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| RangeFileError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        RangeFile::from_toml(&text)
    }

    /// Reads a range file, checking every field. Errors come in the order of the layout, except
    /// that the format version is checked first, so that a file from a later format is reported
    /// as such, and unknown keys before missing ones.
    pub fn from_toml(text: &str) -> Result<RangeFile, RangeFileError> {
        let table: Table = toml::from_str(text)?;
        check_format(&table)?;
        let top = Fields::new(&table, "", &KEYS)?;
        let file = RangeFile {
            config: top.text("config")?,
            formation: top.text("formation")?,
            line: top.text("line")?,
            pot: top.amount("pot_bb")?,
            effective_stack: top.amount("effective_stack_bb")?,
            source: top.text("source")?,
            oop: top.player("oop")?,
            ip: top.player("ip")?,
        };
        check_distinct(&file.oop, &file.ip)?;
        Ok(file)
    }

    /// The file as TOML, in the layout the [module documentation](self) shows. Reading the text
    /// back gives an equal file, for any file that [`RangeFile::from_toml`] returned or that
    /// passes [`RangeFile::check`].
    pub fn to_toml(&self) -> String {
        self.to_string()
    }

    /// The range of one player: [`Player::Oop`] for `[oop]` and [`Player::Ip`] for `[ip]`.
    pub fn player(&self, player: Player) -> &PlayerRange {
        match player {
            Player::Oop => &self.oop,
            Player::Ip => &self.ip,
        }
    }

    /// Checks the file against the game config it names, for a caller that has that config.
    ///
    /// The checks [`RangeFile::from_toml`] makes on values come first (blank text, amounts,
    /// empty ranges, the same seat twice), so that a file built in code can be checked before it
    /// is written. Then:
    ///
    /// - `config` must be the config's [`GameConfig::name`], ignoring case: `a` for config A.
    /// - Both positions must exist at the config's table.
    /// - The `[oop]` player must act before the `[ip]` player after the flop, by
    ///   [`GameConfig::postflop_order`]. A hand-typed file with the two tables swapped fails here.
    /// - The amounts must be reachable. Every seat has the same live stack, so both players have
    ///   put in the same chips, the live stack less the effective stack. That is at least the
    ///   effective blind, since both called it or more. The pot then holds the antes, both
    ///   players' chips and every other seat's folded chips: at least its blind, and at most as
    ///   much as either player put in.
    pub fn check(&self, config: &GameConfig) -> Result<(), RangeFileError> {
        self.check_fields()?;

        if !self.config.eq_ignore_ascii_case(&config.name) {
            let reason = format!(
                "is `{}`, but the game config is named {}",
                self.config, config.name
            );
            return Err(invalid("config", reason));
        }

        let table = config.positions();
        for (key, player) in [("oop", &self.oop), ("ip", &self.ip)] {
            if !table.contains(&player.position) {
                let reason = format!(
                    "is {}, which config {} does not seat; its positions are {}",
                    player.position,
                    config.name,
                    names(table)
                );
                return Err(invalid(format!("{key}.position"), reason));
            }
        }

        let order = config.postflop_order();
        let seat = |position: Position| {
            order
                .iter()
                .position(|&p| p == position)
                .expect("every seat at the table acts after the flop")
        };
        if seat(self.oop.position) > seat(self.ip.position) {
            let reason = format!(
                "is {}, which acts after {} (`ip.position`) on the flop; the `[oop]` and `[ip]` \
                 tables look swapped",
                self.oop.position, self.ip.position
            );
            return Err(invalid("oop.position", reason));
        }

        // Bounds in u64, so that no sum can overflow whatever the config holds.
        let live = u64::from(config.live_stack());
        let blind = u64::from(config.effective_blind());
        let behind = u64::from(self.effective_stack);
        if behind + blind > live {
            let reason = format!(
                "is {}bb, but under config {} a player who sees the flop has put in at least the \
                 {}bb effective blind from a {}bb live stack, so at most {}bb is left",
                bb(behind),
                config.name,
                bb(blind),
                bb(live),
                bb(live - blind)
            );
            return Err(invalid("effective_stack_bb", reason));
        }

        let put_in = live - behind;
        let others = table
            .iter()
            .filter(|&&p| p != self.oop.position && p != self.ip.position);
        let folded_blinds: u64 = others.map(|&p| u64::from(config.blind(p))).sum();
        let dead = u64::from(config.dead_money());
        let least = dead + 2 * put_in + folded_blinds;
        let most = dead + table.len() as u64 * put_in;
        let pot = u64::from(self.pot);
        if !(least..=most).contains(&pot) {
            let reason = format!(
                "is {}bb, but under config {} a pot in which both players have put in {}bb \
                 (leaving {}bb behind) holds from {}bb to {}bb, antes and folded blinds included",
                bb(pot),
                config.name,
                bb(put_in),
                bb(behind),
                bb(least),
                bb(most)
            );
            return Err(invalid("pot_bb", reason));
        }
        Ok(())
    }

    /// The checks [`RangeFile::from_toml`] makes on values, for a file built in code.
    fn check_fields(&self) -> Result<(), Invalid> {
        check_text("config", &self.config)?;
        check_text("formation", &self.formation)?;
        check_text("line", &self.line)?;
        check_amount("pot_bb", self.pot)?;
        check_amount("effective_stack_bb", self.effective_stack)?;
        check_text("source", &self.source)?;
        check_range("oop.range", &self.oop.range)?;
        check_range("ip.range", &self.ip.range)?;
        check_distinct(&self.oop, &self.ip)
    }
}

/// The canonical TOML text of the file. [`RangeFile::to_toml`] returns it.
impl fmt::Display for RangeFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "format = {RANGE_FILE_FORMAT}")?;
        writeln!(f, "config = {}", quote(&self.config))?;
        writeln!(f, "formation = {}", quote(&self.formation))?;
        writeln!(f, "line = {}", quote(&self.line))?;
        writeln!(f, "pot_bb = {}", decimal_bb(self.pot))?;
        writeln!(
            f,
            "effective_stack_bb = {}",
            decimal_bb(self.effective_stack)
        )?;
        writeln!(f, "source = {}", quote(&self.source))?;
        for (key, player) in [("oop", &self.oop), ("ip", &self.ip)] {
            writeln!(f)?;
            writeln!(f, "[{key}]")?;
            writeln!(f, "position = {}", quote(player.position.name()))?;
            writeln!(f, "range = {}", quote(&player.range.to_string()))?;
        }
        Ok(())
    }
}

fn check_format(table: &Table) -> Result<(), Invalid> {
    let reason = match table.get("format") {
        Some(Value::Integer(RANGE_FILE_FORMAT)) => return Ok(()),
        Some(Value::Integer(version)) => {
            format!("is {version}; this solver reads format {RANGE_FILE_FORMAT} only")
        }
        Some(other) => format!(
            "is {}; it must be the integer {RANGE_FILE_FORMAT}",
            kind(other)
        ),
        None => format!("is missing; a range file starts with `format = {RANGE_FILE_FORMAT}`"),
    };
    Err(Invalid::new("format", reason))
}

/// The fields of the top-level table, or of `[oop]` or `[ip]`. Each is named by its key, after
/// the table's name and a dot inside a table: `pot_bb`, `oop.range`.
struct Fields<'a> {
    table: &'a Table,
    /// `""` at the top level, `"oop."` inside `[oop]`.
    prefix: String,
}

impl<'a> Fields<'a> {
    /// The fields of `table`, which is named `name` or is the top level if `name` is empty.
    /// Rejects any key not in `keys`.
    fn new(table: &'a Table, name: &str, keys: &[&str]) -> Result<Fields<'a>, Invalid> {
        let prefix = if name.is_empty() {
            String::new()
        } else {
            format!("{name}.")
        };
        if let Some(key) = table.keys().find(|key| !keys.contains(&key.as_str())) {
            let reason = if name.is_empty() {
                format!("is not a range-file key; the keys are {}", list(keys))
            } else {
                format!("is not a key of `[{name}]`; its keys are {}", list(keys))
            };
            return Err(Invalid::new(format!("{prefix}{key}"), reason));
        }
        Ok(Fields { table, prefix })
    }

    fn name(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    fn get(&self, key: &str) -> Result<&'a Value, Invalid> {
        self.table
            .get(key)
            .ok_or_else(|| Invalid::new(self.name(key), "is missing"))
    }

    fn string(&self, key: &str) -> Result<&'a str, Invalid> {
        match self.get(key)? {
            Value::String(text) => Ok(text),
            other => Err(Invalid::new(
                self.name(key),
                format!("is {}; it must be a string in quotes", kind(other)),
            )),
        }
    }

    fn text(&self, key: &str) -> Result<String, Invalid> {
        let text = self.string(key)?;
        check_text(&self.name(key), text)?;
        Ok(text.to_string())
    }

    fn amount(&self, key: &str) -> Result<Chips, Invalid> {
        let bb = match self.get(key)? {
            Value::Float(bb) => *bb,
            Value::Integer(bb) => *bb as f64,
            other => {
                let reason = format!("is {}; it must be a number of big blinds", kind(other));
                return Err(Invalid::new(self.name(key), reason));
            }
        };
        if bb <= 0.0 {
            let reason = format!("is {bb}; it must be more than 0");
            return Err(Invalid::new(self.name(key), reason));
        }
        let chips = to_chips(&self.name(key), bb)?;
        if chips == 0 {
            let reason = format!("is {bb}bb, less than one chip; the smallest amount is 0.01bb");
            return Err(Invalid::new(self.name(key), reason));
        }
        check_amount(&self.name(key), chips)?;
        Ok(chips)
    }

    fn player(&self, key: &str) -> Result<PlayerRange, Invalid> {
        let table = match self.get(key)? {
            Value::Table(table) => table,
            other => {
                let reason = format!(
                    "is {}; it must be a table, `[{key}]`, holding {}",
                    kind(other),
                    list(&PLAYER_KEYS)
                );
                return Err(Invalid::new(self.name(key), reason));
            }
        };
        let fields = Fields::new(table, &self.name(key), &PLAYER_KEYS)?;

        let name = fields.string("position")?;
        let position: Position = name.trim().parse().map_err(|_| {
            let every = Position::table(9).expect("a 9-handed table exists");
            let reason = format!(
                "is `{name}`, which is not a position; the positions are {}",
                names(every)
            );
            Invalid::new(fields.name("position"), reason)
        })?;

        let text = fields.string("range")?;
        let range: Range = text.parse().map_err(|err| {
            Invalid::new(fields.name("range"), format!("is not a valid range: {err}"))
        })?;
        check_range(&fields.name("range"), &range)?;
        Ok(PlayerRange { position, range })
    }
}

fn check_text(field: &str, text: &str) -> Result<(), Invalid> {
    if text.trim().is_empty() {
        return Err(Invalid::new(field, "is blank"));
    }
    Ok(())
}

fn check_amount(field: &str, chips: Chips) -> Result<(), Invalid> {
    if chips == 0 {
        return Err(Invalid::new(field, "is 0; it must be more than 0"));
    }
    if chips > MAX_CHIPS {
        let reason = format!(
            "is {}bb; an amount must be at most {MAX_BB}bb",
            bb(chips.into())
        );
        return Err(Invalid::new(field, reason));
    }
    Ok(())
}

fn check_range(field: &str, range: &Range) -> Result<(), Invalid> {
    if range.is_empty() {
        return Err(Invalid::new(
            field,
            "holds no hands; each player needs at least one",
        ));
    }
    Ok(())
}

fn check_distinct(oop: &PlayerRange, ip: &PlayerRange) -> Result<(), Invalid> {
    if oop.position == ip.position {
        let reason = format!(
            "is {}, the same seat as `oop.position`; the two players must be in different seats",
            ip.position
        );
        return Err(Invalid::new("ip.position", reason));
    }
    Ok(())
}

/// What kind of TOML value `value` is, for error messages.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "a string",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a decimal number",
        Value::Boolean(_) => "a boolean",
        Value::Datetime(_) => "a date",
        Value::Array(_) => "an array",
        Value::Table(_) => "a table",
    }
}

/// `SB, BB, BTN`.
fn names(positions: &[Position]) -> String {
    let names: Vec<&str> = positions.iter().map(|p| p.name()).collect();
    names.join(", ")
}

/// `a`, `b` and `c`.
fn list(keys: &[&str]) -> String {
    let quoted: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => quoted.concat(),
    }
}

/// An amount in big blinds, exactly and without trailing zeros: `5.5`, `97.25` or `100`.
fn bb(chips: u64) -> String {
    let per_bb = u64::from(CHIPS_PER_BB);
    let (whole, cents) = (chips / per_bb, chips % per_bb);
    match cents {
        0 => whole.to_string(),
        tenths if tenths % 10 == 0 => format!("{whole}.{}", tenths / 10),
        cents => format!("{whole}.{cents:02}"),
    }
}

/// An amount in big blinds as a TOML decimal, which always has a decimal point: `5.5`, `97.25`
/// or `100.0`.
fn decimal_bb(chips: Chips) -> String {
    let text = bb(chips.into());
    if text.contains('.') {
        text
    } else {
        text + ".0"
    }
}

/// `text` as a one-line TOML basic string, in double quotes with escapes.
fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests;
