# Contributing to Vex

Thanks for your interest in Vex. The project is an alpha interpreter and
typed-IR prototype, so focused experiments, tests, documentation, and
language-design feedback are all useful contributions.

## Before you start

1. Read the [README](README.md) and
   [completed phases](README_COMPLETED_PHASES.md).
2. Check the [milestone plan](README_MILESTONES.md) before proposing new
   syntax or runtime behavior.
3. Search existing issues and pull requests so work is not duplicated.
4. For significant language changes, open an issue describing the syntax,
   semantics, diagnostics, and compatibility impact before implementing.

## Good first contributions

- Add parser, evaluator, or diagnostic regression tests.
- Improve language-reference examples.
- Improve error messages and source suggestions.
- Add examples for existing CLI commands.
- Triage documentation gaps and reproducibility issues.
- Implement a narrowly scoped item from the current roadmap.

## Development setup

```sh
git clone https://github.com/cybercoretech/vexlang.git
cd vexlang
cargo test --all-targets
mdbook test docs
```

The minimum supported Rust version is **1.85.0**. Before opening a pull
request, run the complete quality gates:

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo +1.85.0 test --all-targets
cargo +1.85.0 clippy --all-targets -- -D warnings
mdbook test docs
git diff --check
```

## Pull requests

- Keep changes focused and explain the user-visible behavior.
- Add tests for every new language or CLI behavior.
- Update the README, mdBook, changelog, or roadmap when behavior changes.
- Do not emit incomplete compiler output or silently accept unsupported syntax.
- Include the validation commands you ran in the pull request description.
- Keep commits small enough to review.

## Language changes

Every language change should define:

- accepted syntax and precedence;
- runtime and type-checking behavior;
- scope and mutation rules;
- diagnostics for invalid programs;
- interpreter and IR behavior;
- compatibility impact and a migration path, if applicable.

## Community

The current project contact is **`darkstar_dev` on Discord**. A dedicated
server and invite link are not established yet; once available, the invite
will be added here and to the README.

Until then, use GitHub issues and discussions for public project questions,
feature proposals, and contribution coordination. Use the address in
[SECURITY.md](SECURITY.md) for private security reports.

### Discord channel copy

When the community server is created, these messages can be pasted into the
first channels:

#### `#welcome`

```text
⚡ Welcome to VEX // CYBERCORE

Vex is an alpha interpreter and typed-IR prototype exploring syntax, semantics,
diagnostics, and eventual compilation.

Start here:
• Project: https://github.com/cybercoretech/vexlang
• Contributor guide: https://github.com/cybercoretech/vexlang/blob/main/CONTRIBUTING.md
• Docs: https://github.com/cybercoretech/vexlang/tree/main/docs
• Discord contact: darkstar_dev

Discuss language design, interpreter/compiler development, tests, docs,
examples, demos, and constructive feedback.

New here? Read the README, check the roadmap, introduce yourself in #general,
and look for beginner-friendly work in #good-first-issues.

Vex is early-stage: APIs may change and build currently emits textual IR only.
Thanks for helping build it carefully!
```

#### `#rules`

```text
🛡️ VEX // CYBERCORE — COMMUNITY RULES

1. Be constructive and respectful. Critique ideas and code, never people.
2. Welcome newcomers; explain context and make room for questions.
3. Keep discussion on-topic and use the appropriate channel.
4. Do not present alpha behavior as stable or production-ready.
5. Never post security vulnerabilities publicly. Report them privately to
   security@cybercoretech.net with version, reproduction steps, and impact.
6. No spam, harassment, discrimination, impersonation, or malicious content.
7. Respect licenses, authorship, privacy, and other contributors' work.
8. Summarize important decisions in GitHub issues, discussions, PRs, or docs.

Moderators may remove content or restrict access to protect the community.
Questions or concerns? Contact darkstar_dev privately.
```

## Code of conduct

Be constructive, specific, and respectful. Review ideas and code on their
technical merits, welcome newcomers, and assume good intent while asking for
clarification when requirements are unclear.
