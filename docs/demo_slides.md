# LitScout-RS Demo Slides

## 1. Motivation

- Course project goal: a runnable Rust research scouting CLI.
- User gives a technical topic.
- LitScout-RS returns a source-linked Markdown report.

## 2. Data Sources

- GitHub repository search API.
- arXiv Atom API.
- Optional Stage A academic extras: Semantic Scholar, DBLP, OpenAlex, Crossref.
- No arbitrary web crawling, browser automation, or PDF parsing.

## 3. Workflow

```text
topic
-> optional DeepSeek SearchPlan
-> GitHub/arXiv concurrent fetch
-> optional Stage A academic extra fetch
-> normalize SourceItem
-> canonical merge, rank, classify
-> EvidenceQualityGate
-> CitationLedger
-> optional DeepSeek synthesis and repair
-> Markdown report and session JSON
```

## 4. Rust Technical Points

- `tokio::join!` for concurrent source requests.
- `reqwest` for HTTP.
- `serde` for JSON/session/cache/LLM output.
- `roxmltree` for arXiv Atom XML.
- `clap` for CLI.
- `thiserror` for typed errors.
- Local JSON cache and session records.

## 5. LLM Harness

- DeepSeek receives only structured accepted evidence from GitHub/arXiv and explicitly enabled academic extras.
- LLM output must use CitationLedger URLs.
- One repair prompt is allowed if citation validation fails.
- Missing API key returns a clear config error.

## 6. Demo Commands

```bash
cargo run -- "rust agent framework"
DEEPSEEK_API_KEY=... cargo run -- "llm tool calling" --llm
```

## 7. Remaining Optional Work

- `--enrich` README/topics fetch.
- JSON/HTML/PDF export.
- TUI.
