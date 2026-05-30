# LitScout-RS TODO

## Stage 1-3 Status

- [x] Read project plan.
- [x] Create implementation notes.
- [x] Create Rust CLI skeleton.
- [x] Define core models.
- [x] Implement GitHub/arXiv to `SourceItem` conversion.
- [x] Add conversion unit tests.

## Next Required MVP Tasks

- [x] Implement GitHub repository search via official API.
- [x] Implement arXiv API fetch.
- [x] Implement arXiv Atom XML parser with `roxmltree`.
- [x] Add fixture tests for GitHub JSON and arXiv XML.
- [x] Implement exact deduplication.
- [x] Implement rule ranking with stars log scaling.
- [x] Implement rule classification.
- [x] Implement JSON cache with 24h TTL.
- [x] Generate full Markdown report from real source items.
- [x] Add quality gate checks for source counts and source coverage.
- [x] Wire main workflow with concurrent GitHub/arXiv fetch and partial success.

## Stage 5-6 Review Issues (see REVIEW_STAGE_5_6.md)

- [x] **M1**: `incomplete_results` field silently ignored in GitHub search response
- [x] **M2**: Raw topic may be misinterpreted as GitHub search qualifier (add comment or fix)
- [x] **L1**: Add `&#xA0;` (non-breaking space) decoding in arXiv `decode_common_entities`
- [x] **L2**: Remove dead code `_required_headers()` function in `github.rs`
- [ ] **L3**: Extract shared `/abs/` URL stripping utility (refactoring, non-urgent)

## Optional Enhancements

- [ ] `--llm` SearchPlan generation.
- [x] `--llm` report synthesis.
- [ ] LLM citation quality repair, max one retry.
- [x] DeepSeek OpenAI-compatible chat completion call.
- [ ] `--enrich` GitHub README/topics fetch with bounded concurrency.
- [ ] JSON export.
- [ ] TUI.

## Risks

- GitHub rate limits may require a token and cache.
- arXiv XML namespaces need careful parser tests.
- LLM output must never replace source-grounded report data.
- `--llm` currently synthesizes after deterministic retrieval; it does not yet generate SearchPlan query variants.
- Future enrich mode must either bypass cache or extend cache keys to include enrichment settings.
