# LitScout-RS

LitScout-RS is a Rust CLI research scouting tool for GitHub repositories and arXiv papers. It takes a technical topic, fetches metadata from the official GitHub and arXiv APIs, deduplicates and ranks the results, classifies them with simple rules, and writes a Markdown report with source links.

## Usage

```bash
cargo run -- "rust agent framework"
cargo run -- "retrieval augmented generation" --github-limit 10 --arxiv-limit 10
cargo run -- "code agent benchmark" --output reports/agent.md
cargo run -- "rag" --no-cache
cargo run -- "llm tool calling" --llm
cargo run -- "rust agent framework" --enrich
```

Useful environment variables:

```bash
export GITHUB_TOKEN=...
export DEEPSEEK_API_KEY=...
```

`GITHUB_TOKEN` is optional but recommended to reduce GitHub API rate-limit issues.

## DeepSeek LLM Agent Mode

`--llm` enables the DeepSeek-backed analysis layer after GitHub and arXiv data have already been collected, deduplicated, ranked, classified, and converted into the local `CitationLedger`.

DeepSeek only receives the structured GitHub/arXiv context produced by LitScout-RS. It is instructed not to browse, invent sources, or add URLs outside the citation ledger. If its output drops source URLs or references unknown citations, the quality gate records a warning or the synthesis is rejected.

Configure with environment variables:

```bash
export DEEPSEEK_API_KEY=...
export DEEPSEEK_BASE_URL=https://api.deepseek.com
export DEEPSEEK_MODEL=deepseek-v4-pro
export DEEPSEEK_SIDE_MODEL=deepseek-v4-flash
export DEEPSEEK_MAX_TOKENS=4096
export DEEPSEEK_TIMEOUT_SECS=30
```

Or pass CLI arguments:

```bash
cargo run -- "rust agent framework" --llm \
  --deepseek-api-key "$DEEPSEEK_API_KEY" \
  --deepseek-base-url https://api.deepseek.com \
  --deepseek-model deepseek-v4-pro \
  --deepseek-side-model deepseek-v4-flash \
  --llm-timeout 45 \
  --llm-max-tokens 4096
```

Short form:

```bash
DEEPSEEK_API_KEY=... cargo run -- "llm tool calling" --llm
```

If `--llm` is enabled without an API key, the program returns a clear configuration error instead of pretending that LLM synthesis succeeded.

Reports are written to `reports/<topic>-<timestamp>.md` by default. Use `--output reports/agent.md` to write a fixed file path.

## Development Checks

```bash
cargo fmt
cargo check
cargo test
```
