#!/usr/bin/env python3
"""Generate site/keywords.html from VEXLANG_KEYWORDS_PROPOSAL.md.

The proposal document is the single source of truth for the keyword catalogue.
This script parses it, cross-references src/vex.pest to work out which
constructs are actually reachable in the current build, and renders the page.

Status values:
  live      parsed, built and evaluable in the current alpha
  reserved  has a grammar rule, but the builder rejects it today
  proposed  reserved word in the grammar's identifier guard, no syntax yet
  idea      proposed in the document only, not reserved in the grammar

Usage:  python3 scripts/build-keywords.py
"""

import html
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROPOSAL = ROOT / "VEXLANG_KEYWORDS_PROPOSAL.md"
GRAMMAR = ROOT / "src" / "vex.pest"
OUT = ROOT / "site" / "keywords.html"

# Constructs that parse *and* build in the current alpha. Verified against the
# interpreter with scripts/check-keyword-status.sh.
LIVE = {
    "let", "fn", "if", "else", "while", "break", "continue", "return",
    "record", "enum", "match", "trait", "module", "use", "export",
    "contract", "invariant", "newtype", "unsafe", "ensure", "throw",
}

# Grammar rules exist (src/vex.pest `stmt`), but the builder rejects them.
RESERVED_RULE = {"impl", "test", "bench", "extern", "trusted"}

STATUS_LABEL = {
    "live": "LIVE",
    "reserved": "RESERVED",
    "proposed": "PROPOSED",
}

STATUS_DETAIL = {
    "live": "Parses, builds and evaluates in 0.1.0-alpha.1",
    "reserved-rule": "Has a grammar rule; the builder rejects it today",
    "reserved-word": "Reserved identifier in the grammar; no syntax yet",
    "proposed": "Proposal only; not reserved in the grammar",
}

TYPES = {"i32", "u64", "f64", "bool", "string", "res", "u8", "unit"}


def grammar_words() -> set[str]:
    """Every KW_* literal in the grammar, i.e. words that cannot be identifiers."""
    text = GRAMMAR.read_text(encoding="utf-8")
    return {m.lower() for m in re.findall(r'KW_[A-Z0-9_]+ = _\{ "([a-z0-9_]+)" \}', text)}


def parse_proposal(text: str) -> list[dict]:
    """Split the document into per-keyword records."""
    entries = []
    category = None
    # `## Category 1: Memory & Resource Management (8 items)`
    cat_re = re.compile(r"^##\s+Category\s+(\d+):\s*(.+?)\s*\((\d+)\s*items?\)\s*$")
    # `### 1. \`arena\` - Arena Allocator`
    kw_re = re.compile(r"^###\s+(\d+)\.\s+`([^`]+)`\s*-\s*(.+?)\s*$")

    blocks = re.split(r"^(?=###\s+\d+\.)", text, flags=re.MULTILINE)
    for block in blocks:
        for line in block.splitlines():
            m = cat_re.match(line)
            if m:
                category = {
                    "index": int(m.group(1)),
                    "name": m.group(2),
                    "count": int(m.group(3)),
                }
                break
        m = kw_re.match(block.splitlines()[0]) if block.strip() else None
        if not m or category is None:
            continue
        entries.append({
            "number": int(m.group(1)),
            "keyword": m.group(2).strip(),
            "name": m.group(3).strip(),
            "category": category["index"],
            "category_name": category["name"],
            "purpose": field(block, "Purpose"),
            "syntax": field(block, "Syntax"),
            "why": field(block, "Why"),
            "example": example(block),
        })
    return entries


def field(block: str, name: str) -> str:
    m = re.search(rf"^\*\*{name}:\*\*\s*(.+)$", block, re.MULTILINE)
    if not m:
        return ""
    return m.group(1).strip()


def example(block: str) -> str:
    m = re.search(r"^\*\*Example:\*\*\s*$.*?```vex\n(.*?)```", block, re.MULTILINE | re.DOTALL)
    if m:
        return m.group(1).rstrip()
    m = re.search(r"```vex\n(.*?)```", block, re.DOTALL)
    return m.group(1).rstrip() if m else ""


def status_for(keyword: str, reserved_words: set[str]) -> str:
    """Return the filter bucket: live, reserved or proposed."""
    detail = detail_for(keyword, reserved_words)
    if detail == "live":
        return "live"
    if detail.startswith("reserved"):
        return "reserved"
    return "proposed"


def detail_for(keyword: str, reserved_words: set[str]) -> str:
    if keyword in LIVE:
        return "live"
    if keyword in RESERVED_RULE:
        return "reserved-rule"
    if keyword in reserved_words:
        return "reserved-word"
    return "proposed"


def highlight(code: str, keywords: set[str]) -> str:
    """Tiny Vex highlighter: comments, strings, numbers, keywords, types."""
    token = re.compile(
        r'(//[^\n]*)'          # comment
        r'|("(?:[^"\\]|\\.)*")'  # string
        r'|(\b\d+\b)'          # number
        r'|([A-Za-z_][A-Za-z0-9_]*)'  # word
    )
    out, pos = [], 0
    for m in token.finditer(code):
        out.append(esc(code[pos:m.start()]))
        comment, string, number, word = m.groups()
        if comment:
            out.append(f'<span class="t-com">{esc(comment)}</span>')
        elif string:
            out.append(f'<span class="t-str">{esc(string)}</span>')
        elif number:
            out.append(f'<span class="t-num">{esc(number)}</span>')
        elif word in keywords:
            out.append(f'<span class="t-kw">{esc(word)}</span>')
        elif word in TYPES:
            out.append(f'<span class="t-ty">{esc(word)}</span>')
        else:
            out.append(esc(word))
        pos = m.end()
    out.append(esc(code[pos:]))
    return "".join(out)


def slug(keyword: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", keyword.lower()).strip("-")


def esc(text: str) -> str:
    return html.escape(text, quote=True)


def main() -> int:
    if not PROPOSAL.exists():
        print(f"missing {PROPOSAL}", file=sys.stderr)
        return 1

    reserved_words = grammar_words()
    entries = parse_proposal(PROPOSAL.read_text(encoding="utf-8"))
    if len(entries) != 150:
        print(f"expected 150 keywords, parsed {len(entries)}", file=sys.stderr)
        return 1

    keyword_set = set()
    for entry in entries:
        entry["status"] = status_for(entry["keyword"], reserved_words)
        entry["detail"] = detail_for(entry["keyword"], reserved_words)
        entry["slug"] = slug(entry["keyword"])

    categories = []
    for entry in entries:
        if not categories or categories[-1]["index"] != entry["category"]:
            categories.append({
                "index": entry["category"],
                "name": entry["category_name"],
                "items": [],
            })
        categories[-1]["items"].append(entry)

    raw_counts = {}
    for entry in entries:
        raw_counts[entry["status"]] = raw_counts.get(entry["status"], 0) + 1
    counts = {
        "live": raw_counts.get("live", 0),
        "reserved": raw_counts.get("reserved", 0),
        "proposed": raw_counts.get("proposed", 0),
    }

    data = {
        "keywords": entries,
        "categories": categories,
        "counts": counts,
    }

    OUT.write_text(render(data), encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)} — {len(entries)} keywords, {len(categories)} categories, {counts}")
    return 0


def render(data: dict) -> str:
    entries = data["keywords"]
    categories = data["categories"]
    counts = data["counts"]

    payload = json.dumps(
        [
            {
                "n": e["number"],
                "k": e["keyword"],
                "name": e["name"],
                "c": e["category"],
                "s": e["status"],
                "slug": e["slug"],
            }
            for e in entries
        ],
        separators=(",", ":"),
    )

    keyword_set = {e["keyword"].lower() for e in entries}

    cards = []
    for e in entries:
        st = e["status"]
        label = STATUS_LABEL[st]
        detail = e.get("detail", "")
        highlighted = highlight(e["example"], keyword_set)
        cards.append(f"""
      <article class="kw" id="{e['slug']}" data-keyword="{esc(e['keyword'])}" data-status="{st}" data-category="{e['category']}" data-name="{esc(e['name'])}" data-purpose="{esc(e['purpose'])}" data-detail="{esc(detail)}">
        <header class="kw-head">
          <span class="kw-num">{e['number']:03d}</span>
          <code class="kw-name">{esc(e['keyword'])}</code>
          <span class="kw-status s-{st}">{label}</span>
        </header>
        <h3 class="kw-title">{esc(e['name'])}</h3>
        <p class="kw-purpose">{esc(e['purpose'])}</p>
        <p class="kw-syntax"><span>SYNTAX</span><code>{esc(e['syntax'])}</code></p>
        <pre class="kw-code"><code>{highlighted}</code></pre>
        <p class="kw-why"><span>WHY</span>{esc(e['why'])}</p>
      </article>""")

    sections = []
    for cat in categories:
        items = "".join(c for c, e in zip(cards, entries) if e["category"] == cat["index"])
        sections.append(f"""
    <section class="cat" id="cat-{cat['index']}" data-category="{cat['index']}">
      <div class="cat-head">
        <span class="cat-index">CATEGORY {cat['index']:02d}</span>
        <h2>{esc(cat['name'])}</h2>
        <span class="cat-count">{len(cat['items'])} constructs</span>
      </div>
      <div class="kw-grid">{items}
      </div>
    </section>""")

    chips = "".join(
        f'<button class="chip" type="button" data-cat="{c["index"]}">{esc(c["name"])}<b>{len(c["items"])}</b></button>'
        for c in categories
    )

    return f"""<!doctype html>
<html lang="en" data-theme="neon-night">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="theme-color" content="#080d15">
<meta name="description" content="All 150 proposed VexLang language constructs — syntax, rationale and implementation status for each keyword.">
<title>VexLang // Keyword Index</title>
<link rel="icon" type="image/svg+xml" href="assets/icons/vex-mark.svg">
<link rel="icon" type="image/png" sizes="32x32" href="assets/icons/favicon-32.png">
<link rel="icon" type="image/png" sizes="16x16" href="assets/icons/favicon-16.png">
<link rel="apple-touch-icon" href="assets/icons/apple-touch-icon.png">
<link rel="manifest" href="assets/site.webmanifest">
<meta property="og:title" content="VexLang // Keyword Index">
<meta property="og:description" content="All 150 proposed VexLang language constructs — syntax, rationale and implementation status for each keyword.">
<meta property="og:type" content="website">
<meta property="og:image" content="https://cybercoretech.github.io/vexlang/assets/og/og-image.png">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="VexLang // Keyword Index">
<meta name="twitter:description" content="All 150 proposed VexLang language constructs — syntax, rationale and implementation status for each keyword.">
<meta name="twitter:image" content="https://cybercoretech.github.io/vexlang/assets/og/og-image.png">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600;700&display=swap" rel="stylesheet">
<link rel="stylesheet" href="theme/cybercore.css">
<link rel="stylesheet" href="theme/keywords.css">
</head>
<body class="kw-page">
<a class="skip" href="#main">Skip to content</a>
<header class="shell header">
  <a class="brand-lockup" href="./" aria-label="VexLang home">
    <span class="brand-mark">V</span><span class="brand-name">EXLANG</span>
  </a>
  <nav class="main-nav" aria-label="Primary navigation">
    <a href="./#language">language</a>
    <a href="./#milestones-section">milestones</a>
    <a href="./#playground">playground</a>
    <a href="keywords.html" aria-current="page">keywords</a>
    <a href="docs/index.html">docs</a>
    <a href="https://github.com/cybercoretech/vexlang" target="_blank" rel="noopener noreferrer">SOURCE ↗</a>
  </nav>
  <button class="theme-trigger" type="button" data-theme-open aria-haspopup="dialog"><span class="theme-dot"></span><span>CYBERCORE THEME</span><b id="theme-current">—</b><span aria-hidden="true">⌄</span></button>
</header>
<main id="main">

<section class="shell kw-hero">
  <div class="eyebrow">CYBERCORE LANGUAGE LAB / KEYWORD INDEX</div>
  <h1>150 constructs.<br><span>One proposal.</span></h1>
  <p class="kw-lede">Every keyword VexLang has proposed for itself — what it does, the syntax it would take, and why it earned a place on the list. Each card is tagged with what the current alpha actually does with it, so the catalogue never promises more than the build delivers.</p>
  <div class="kw-stats">
    <div class="stat"><b>{len(entries)}</b><span>constructs</span></div>
    <div class="stat"><b>{len(categories)}</b><span>categories</span></div>
    <div class="stat live"><b>{counts['live']}</b><span>live today</span></div>
    <div class="stat reserved"><b>{counts['reserved']}</b><span>reserved</span></div>
    <div class="stat proposed"><b>{counts['proposed']}</b><span>proposed</span></div>
  </div>
  <p class="kw-note"><b>READ THIS FIRST.</b> This page documents a design proposal, not the shipped language. <b class="s-live">LIVE</b> constructs parse, build and evaluate in <code>0.1.0-alpha.1</code>. <b class="s-reserved">RESERVED</b> words have grammar rules but are rejected by the builder. <b class="s-proposed">PROPOSED</b> constructs are reserved words or design ideas with no implementation yet.</p>
</section>

<section class="shell kw-tools" aria-label="Filter keywords">
  <div class="tool-row">
    <label class="search">
      <span class="search-icon" aria-hidden="true">⌕</span>
      <input id="kw-search" type="search" placeholder="Search keywords, syntax or rationale…" autocomplete="off" spellcheck="false">
      <kbd>/</kbd>
    </label>
    <div class="status-filter" role="group" aria-label="Filter by status">
      <button class="seg is-on" type="button" data-status="all">ALL <b>{len(entries)}</b></button>
      <button class="seg" type="button" data-status="live">LIVE <b>{counts['live']}</b></button>
      <button class="seg" type="button" data-status="reserved">RESERVED <b>{counts['reserved']}</b></button>
      <button class="seg" type="button" data-status="proposed">PROPOSED <b>{counts['proposed']}</b></button>
    </div>
  </div>
  <div class="chips" id="kw-chips">
    <button class="chip is-on" type="button" data-cat="all">All categories<b>{len(entries)}</b></button>
    {chips}
  </div>
  <p class="result-line"><span id="kw-count">{len(entries)}</span> constructs shown <button id="kw-reset" type="button" hidden>clear filters</button></p>
</section>

<div class="shell kw-body" id="kw-list">
{sections}
</div>

<section class="shell kw-empty" id="kw-empty" hidden>
  <p class="eyebrow">NO MATCH</p>
  <h2>Nothing on the list answers to that.</h2>
  <p>Try a broader term, or reset the filters.</p>
  <button class="button primary" type="button" id="kw-empty-reset">Reset filters</button>
</section>

<section class="shell kw-cta">
  <div>
    <p class="eyebrow">SHIPPED SURFACE</p>
    <h2>The alpha is smaller than the proposal.</h2>
    <p>Only the live constructs above exist in the interpreter today. The rest live in <code>VEXLANG_KEYWORDS_PROPOSAL.md</code> and <code>src/vex.pest</code>, where several are already reserved so they cannot be used as identifiers.</p>
  </div>
  <a class="button primary" href="docs/language.html">Read the language reference <span>→</span></a>
</section>
</main>

<footer class="shell footer">
  <span>VEXLANG // CYBERCORE LANGUAGE LAB</span>
  <div class="links">
    <a href="./">Home ↗</a>
    <a href="https://github.com/cybercoretech/vexlang">GitHub ↗</a>
    <a href="docs/index.html">Docs ↗</a>
    <a data-cybercore-master-docs href="https://cybercoretech.github.io/cybercore/docs.html">Master docs ↗</a>
    <span>MIT · 0.1.0-alpha.1</span>
  </div>
</footer>

<script>window.VEXLANG_KEYWORD_INDEX={payload};</script>
<script src="theme/theme-catalog.js"></script>
<script src="theme/cybercore.js"></script>
<script src="scripts/keywords.js"></script>
</body>
</html>
"""


if __name__ == "__main__":
    raise SystemExit(main())
