//! ## Vex [Vexlang] ##
//! # Version: 0.0.1 (SemVer)
//! # Inspired by Rust & Zig
//! # Built for Cybercore ecosystem code consistency and ease
//! # GitHub: [Vexlang] (https://github.com/cybercoretech/vexlang)
//! # Tag reference doc in /home/raven/devspace/docs/tags/TAG_API.md
//!

use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "vex.pest"]
pub struct VexParser;

pub fn parse_vex(
    source: &str,
) -> Result<pest::iterators::Pairs<'_, Rule>, pest::error::Error<Rule>> {
    use pest::Parser;
    VexParser::parse(Rule::program, source)
}
