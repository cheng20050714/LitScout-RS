# LitScout-RS Implementation Notes

## Current Scope

This implementation follows `../PROJECT_PLAN.md` and starts with a CLI-first MVP.

The current build covers:

- Rust CLI application.
- Module layout for config, errors, models, workflow, sources, ranking, classification, deduplication, cache, report, quality, and DeepSeek LLM synthesis.
- Core data structures for GitHub repositories, arXiv papers, unified source items, citations, reports, quality checks, and LLM config.
- Conversion from `GitHubRepo` and `ArxivPaper` into `SourceItem`.
- GitHub repository search through the official REST API.
- arXiv search through the official Atom API with XML parsing.
- Local JSON cache keyed by query, source, and limit.
- Exact stable-ID deduplication, rule ranking, rule classification, citation ledger, and Markdown report rendering.
- Main workflow with concurrent GitHub/arXiv fetching and partial success.
- Optional DeepSeek-backed report synthesis through an OpenAI-compatible chat completions request.
- LLM output validation that rejects missing citation IDs, dropped source URLs, and URLs outside the citation ledger.
- Unit tests and fixture tests for models, parsers, cache, ranking, classification, report rendering, quality gates, and no-network workflow execution.

## MVP Boundary

The target MVP is:

1. Parse a user topic from CLI.
2. Concurrently query GitHub and arXiv.
3. Parse repository and paper metadata.
4. Normalize into `SourceItem`.
5. Deduplicate, rank, classify.
6. Generate a Markdown report with source links.
7. Support cache, logging, and graceful errors.
8. Keep LLM support optional.

## Current Strategy

The MVP path is now implemented as a CLI-first pipeline:

1. Build a `SearchQuery` from CLI/config.
2. Fetch GitHub and arXiv concurrently, using cache when enabled.
3. Continue with partial results when one source fails.
4. Normalize to `SourceItem`.
5. Deduplicate, rank, classify, and group by topic labels.
6. Build citations and run quality checks.
7. If `--llm` is enabled, call DeepSeek with only the structured source context and parse the returned JSON into `LlmSynthesis`.
8. Write a source-linked Markdown report.

LLM support remains optional. `--llm` requires `DEEPSEEK_API_KEY` or `--deepseek-api-key`; without a key the program returns an explicit configuration error. If DeepSeek is called but synthesis fails, the deterministic report is still written with a warning.

## Explicit Non-Goals For This Stage

- No Web UI.
- No browser agent.
- No arbitrary web crawler.
- No PDF full-text parser.
- No multi-turn ReAct loop.
- No README/topics enrichment by default.
- No LLM-generated SearchPlan yet.
- No LLM repair prompt yet.
