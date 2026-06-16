# LitScout-RS Search Boundary Expansion Plan

Planning date: 2026-06-15

This document plans how LitScout-RS should expand beyond the current GitHub and arXiv boundary while preserving query precision, citation traceability, and the controlled-agent design of the existing system. It is documentation-only: no business code, schema migration, dependency, or runtime behavior is changed by this plan.

## 1. Position

LitScout-RS should expand search coverage by adding authoritative, structured, verifiable sources before adding open web search. The target is not a general browser agent. The target is a broader research scouting pipeline where every source enters the same evidence, lineage, ranking, and citation-audit machinery.

The core rule is:

```text
structured source first
  -> verified metadata
  -> unified evidence model
  -> ranked and audited report
  -> open web only as auxiliary corroboration
```

This preserves the current product contract: the LLM may analyze retrieved evidence, but it must not invent external URLs or browse independently.

## 2. Current State

The current mainline is intentionally bounded:

- `GitHubScout` retrieves repositories through the GitHub repository search API and optional README enrichment.
- `ArxivScout` retrieves papers through the arXiv Atom API.
- `EvidenceBuilder` converts both source families into `SourceItem`, `EvidenceItem`, `CitationLedger`, and `SourceQueryLineage`.
- `Writer` drafts the Chinese report from `EvidenceMemory`.
- `CitationAuditor` checks URL whitelist violations, paragraph evidence coverage, and coarse source diversity.

This architecture is suitable for expansion, but several components still encode the old two-source assumption:

- `SourceKind` only has `GitHub` and `Arxiv`.
- `SourceMetadata` is source-specific rather than adapter-oriented.
- `QueryPortfolio` stores `github_queries` and `arxiv_queries` directly.
- `CoverageCritic`, `Writer`, and product copy still describe a GitHub/arXiv-only world.

The expansion should keep the same stateful workflow and replace the hard-coded source pair with an explicit source adapter layer.

## 3. Design Principles

1. **Accuracy before breadth**: a source is not admitted because it has many results; it is admitted because returned records are structured enough to rank, cite, and audit.
2. **Provenance is mandatory**: every source result must be linked to one or more query attempts. No source item may appear in a report without a lineage path.
3. **Open web is auxiliary**: web search can find official blogs, release notes, project pages, benchmark pages, and documentation, but it should not outrank authoritative academic or repository sources by default.
4. **No hidden browsing by the LLM**: the LLM plans queries and synthesizes evidence; deterministic Rust adapters do all network access.
5. **Fail closed for evidence, fail visible for workflow**: if a source fails or returns low-quality records, keep the run alive but mark the gap. Do not silently substitute weaker evidence as if it were equivalent.
6. **Rollout by source class**: add bibliographic sources first, venue/preprint sources second, open web last.

## 4. Target Architecture

The future implementation should introduce a source adapter abstraction at the planning and fetching boundary.

```text
ResearchBrief
  -> Planner
  -> MultiSourceQueryPortfolio
       - chapter_id
       - source_kind
       - query_text
       - budget
       - source_policy
  -> SourceAdapterRegistry
       - GitHub
       - arXiv
       - Semantic Scholar
       - DBLP
       - OpenAlex / Crossref
       - optional domain sources
       - optional web auxiliary source
  -> SourceItem[]
  -> EvidenceMemory + CitationLedger + SourceQueryLineage
  -> CoverageCritic + CitationAuditor
  -> Writer
```

The adapter contract should require these behaviors:

- Accept a normalized query spec, not raw user text only.
- Return normalized source items with stable IDs.
- Preserve source-native metadata in a typed or tagged metadata payload.
- Report rate limits, empty results, parser failures, and partial results as structured query attempts.
- Expose a source-quality tier so ranking and coverage can distinguish authoritative records from web pages.

The implementation should avoid introducing a free-form ReAct loop. Parallelism can remain at the tool/source execution layer, as SearchClaw does for concurrency-safe search and fetch tools.

`source_policy` should be explicit per chapter so coverage checks know what kind of evidence is required:

| Policy | Meaning |
|---|---|
| `require_academic` | The chapter must include at least one academic, preprint, venue, or bibliography source. |
| `require_artifact` | The chapter must include at least one implementation or artifact source, such as GitHub or a future model/dataset hub. |
| `prefer_official` | Official documentation, release notes, project pages, or benchmark pages should be preferred, with academic and implementation sources as support. |
| `academic_only` | Only academic, preprint, venue, or bibliography sources can satisfy coverage. |
| `any_structured` | Any Tier 1 or Tier 2 structured source can satisfy coverage. This is the default for broad technical scouting. |

`required_evidence_kinds` remains the chapter-level coverage contract. It should be derived from `source_policy` plus planner intent, then preserved in the run state so later adapters cannot silently satisfy an academic chapter with web-only evidence.

## 5. Source Expansion Stages

### Stage A: Academic Authority Expansion

Goal: improve paper recall, metadata precision, and ranking quality for CS/AI topics while keeping implementation risk low.

Recommended sources:

- **Semantic Scholar**: broad paper search, citation counts, abstracts, authors, venues, years, and paper IDs.
- **DBLP**: high-quality CS bibliography with precise venue and publication metadata.
- **OpenAlex**: open scholarly graph for works, authors, institutions, sources, DOI metadata, and broader coverage than CS-only bibliographies.
- **Crossref**: DOI-centered metadata validation and fallback for title/DOI/venue matching.

Stage A changes the system from "arXiv-only paper discovery" to "multi-index academic discovery." arXiv remains valuable for preprint full-text and PDF workflows, while Semantic Scholar, DBLP, OpenAlex, and Crossref improve metadata verification and reduce blind spots.

Acceptance standard:

- A technical topic should return at least one useful academic source from two independent academic indexes when available.
- Adding Stage A must not cause low-quality bibliographic records to outrank strong arXiv papers or GitHub repositories without relevance evidence.

### Stage B: Domain and Venue Source Expansion

Goal: improve field-specific coverage when the research topic is outside the default CS/arXiv center of gravity or targets conference proceedings.

Recommended sources:

- **ACL Anthology** for NLP, computational linguistics, speech, and LLM-adjacent topics.
- **OpenReview** for ICLR, NeurIPS, ICML, and other venues hosted on OpenReview.
- **bioRxiv / medRxiv / ChemRxiv** for cross-disciplinary topics.

Stage B should not copy `daily-paper-reader`'s subscription, Supabase, or daily maintenance workflow. The useful idea to migrate is its source separation and multi-source retrieval discipline: each source has its own schema, matching function, tests, and selection policy.

Acceptance standard:

- Domain sources are enabled only when the topic, user-selected source profile, or planner explicitly asks for them.
- Domain records are labeled clearly in the report so users understand whether evidence came from a preprint server, official anthology, or conference platform.

### Stage C: Open Web Auxiliary Expansion

Goal: add non-paper evidence for official announcements, documentation, release notes, benchmark pages, product pages, standards, and project homepages.

Recommended capabilities:

- Search discovery through a search API such as Serper or a best-effort DuckDuckGo HTML fallback.
- Page reading through Jina Reader first, then direct HTTP + local HTML extraction.
- Optional content caching and section-level reread for large pages.

SearchClaw's relevant transferable pattern is not its Python runtime; it is the harness:

- search first, fetch promising pages second;
- store citations automatically, then require explicit citation registration;
- block final answers that have too few citations or too little source diversity;
- cache large content and extract relevant facts before injecting into the LLM context.

SearchClaw migration decisions:

| SearchClaw Capability | Decision | Reason |
|---|---|---|
| `search -> fetch -> cite` control flow | Migrate the design pattern | It is the safest way to prevent snippets from becoming evidence. |
| Concurrency-safe search/fetch tools | Migrate into the Rust adapter layer | LitScout-RS already executes source calls concurrently; adapters should declare whether they are safe to parallelize. |
| Automatic citation discovery plus explicit citation registration | Migrate conceptually | `CitationLedger` already provides the ledger; future web evidence should distinguish discovered from admitted citations. |
| Stop hooks for citation count and source diversity | Migrate as report/evidence gates | They align with existing `CitationAuditor` and coverage checks. |
| Large-page cache plus section-level deep read | Migrate later for Stage C | Useful for web/PDF pages, but not needed for Stage A structured sources. |
| Python runtime and dependency tree | Do not migrate | LitScout-RS should stay Rust-first. |
| Browser/CDP rendering | Do not migrate in the core flow | High maintenance and privacy/setup risk. |
| WeChat/Sogou crawling | Do not migrate | Fragile anti-spider behavior and weak fit with the current research-scouting boundary. |

Open web results should enter an auxiliary evidence lane by default. They can support context, recency, and official statements, but they should not dominate academic/repository ranking unless the chapter is explicitly about current releases, product documentation, or non-academic artifacts.

Acceptance standard:

- Web evidence must include fetched page content, not just search snippets.
- Search engine result pages are never cited.
- Web results must pass URL safety, content extraction, citation whitelist, and domain diversity checks.

## 6. Accuracy Preservation Controls

The expansion should add these controls before or alongside new sources:

- **Source tiering**:
  - Tier 1: official structured APIs and curated bibliographic databases.
  - Tier 2: venue/preprint/domain repositories.
  - Tier 3: official project/documentation/web pages.
  - Tier 4: general news/blog/web pages.
- **Ranking guardrails**:
  - Keep per-source quota so a noisy source cannot flood the global pool.
  - Use reciprocal-rank or lane-guaranteed candidate pools before final rerank, following the useful pattern from `daily-paper-reader`.
  - Treat snippets-only results as discovery hints, not citeable evidence.
- **Citation guardrails**:
  - Writer may only cite `CitationLedger` URLs.
  - Every cited paragraph must map to known `EvidenceItem` IDs.
  - External URLs in generated text remain violations unless present in the ledger.
- **Coverage guardrails**:
  - Distinguish query gaps from source gaps.
  - A chapter that asks for papers should not be considered covered by web pages alone.
  - A chapter that asks for implementation should not be considered covered by papers alone.
- **Freshness guardrails**:
  - For current-events or release-note chapters, record publication/update dates when source APIs provide them.
  - For academic review chapters, recency is a ranking factor but not a hard exclusion unless requested.

## 7. Migration Sequence

### Phase 1: Documentation and Evaluation Design

Produce and review:

- `SEARCH_BOUNDARY_EXPANSION_PLAN.md`
- `SOURCE_ADAPTER_MATRIX.md`
- `ACCURACY_EVALUATION_PROTOCOL.md`

No runtime behavior changes.

### Phase 2: Model and Planning Refactor

Introduce a source-agnostic query portfolio and source kind taxonomy while keeping GitHub/arXiv behavior identical.

Required compatibility rule: existing sessions and reports should remain readable. New fields should use serde defaults where possible.

### Phase 3: Stage A Adapters

Implement Semantic Scholar and DBLP first, then OpenAlex/Crossref as metadata validation or secondary retrieval adapters.

The first implementation should keep all new sources behind explicit CLI/Web settings. The default run can continue using GitHub/arXiv until Stage A passes evaluation.

### Phase 4: Ranking and Coverage Hardening

Add source lanes, per-lane quotas, source tier bonuses/penalties, and regression tests so broad bibliographic sources do not drown out high-signal existing evidence.

### Phase 5: Domain Sources

Add ACL Anthology and OpenReview before biomedical/chemistry preprint sources unless the target users shift toward cross-disciplinary research.

### Phase 6: Open Web Auxiliary Lane

Add web discovery and page fetch only after citation and ranking guardrails are strong enough to prevent snippet-only or low-authority web pages from becoming core evidence.

## 8. Explicit Non-Goals

This expansion should not:

- add a browser automation agent to the default flow;
- let the LLM choose arbitrary URLs to fetch;
- cite search snippets as evidence;
- copy `daily-paper-reader`'s Supabase subscription backend;
- copy SearchClaw's entire Python runtime;
- add WeChat or authenticated CDP browsing to the Rust core in the first implementation wave;
- make news sources part of the default technical research report.

## 9. Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| Broad sources return many weakly related papers | Report precision drops | Per-source quotas, rerank regression, manual precision checks |
| Metadata duplicates across arXiv, Semantic Scholar, DBLP, OpenAlex | Citation ledger duplicates or confusing rankings | Stable IDs by DOI/arXiv ID/title normalization, source provenance merge rules |
| Open web results overwhelm structured sources | Report becomes generic and noisy | Web lane stays auxiliary by default; fetched content required for citation |
| API rate limits interrupt runs | Partial evidence or failed chapters | Structured query attempts, retry/backoff, cache, source-specific budgets |
| LLM writes URLs it saw in fetched pages but ledger did not admit | Citation whitelist failure | Keep current auditor behavior and expand it to all source kinds |
| Domain sources add maintenance burden | Slow development and fragile parsers | Add only after Stage A evaluation; require fixture tests per source |

## 10. Source References Checked

- Semantic Scholar Academic Graph API: <https://api.semanticscholar.org/api-docs/graph>
- arXiv API user manual: <https://info.arxiv.org/help/api/user-manual.html>
- DBLP search API FAQ: <https://dblp.org/faq/13501473.html>
- OpenAlex developer documentation: <https://developers.openalex.org/>
- Crossref REST API tips: <https://www.crossref.org/documentation/retrieve-metadata/rest-api/tips-for-using-the-crossref-rest-api/>
- ACL Anthology development and API: <https://aclanthology.org/info/development/>
- OpenReview API V2 reference: <https://docs.openreview.net/reference/api-v2>
- bioRxiv API: <https://api.biorxiv.org/>
- Jina Reader: <https://jina.ai/reader/>
- NewsAPI Everything endpoint: <https://newsapi.org/docs/endpoints/everything>
- Google robots.txt guide: <https://developers.google.com/search/docs/crawling-indexing/robots/intro>
