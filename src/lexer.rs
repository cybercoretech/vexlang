//! ## Vex [Vexlang] ##
//! # Version: 0.0.1 (SemVer)
//! # Inspired by Rust & Zig
//! # Built for Cybercore ecosystem code consistency and ease
//! # GitHub: [Vexlang] (https://github.com/cybercoretech/vexlang)
//! # Tag reference doc in /home/raven/devspace/docs/tags/TAG_API.md
//!

use logos::Logos;

#[derive(Logos, Debug, PartialEq)]
#[logos(skip r"[ \t\n\f]+")] // Ignore whitespace
pub enum Token {
    #[token("let")] Let,
    #[token("fn")]  Fn,
    #[token("=")]   Assign,
    #[token(";")]   Semicolon,
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")] Identifier,
    #[regex(r"[0-9]+")] Integer,
    #[error] Error,
}
