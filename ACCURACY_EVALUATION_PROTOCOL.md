# Accuracy Evaluation Protocol for Search Boundary Expansion

Planning date: 2026-06-15

This protocol defines how LitScout-RS should evaluate new search sources before they become default evidence providers. The purpose is to expand coverage without degrading report accuracy, citation trust, or user confidence.

## 1. Evaluation Goals

The evaluation should answer four questions for every source expansion:

1. **Does the source improve recall for the intended topic class?**
2. **Does it preserve or improve top-result precision?**
3. **Can every report claim still be traced to admitted evidence?**
4. **Does the new source fail visibly instead of silently lowering evidence quality?**

The protocol treats "more results" as insufficient. A source only passes if it improves useful evidence under the existing controlled-report workflow.

## 2. Required Metrics

| Metric | Definition | Required Use |
|---|---|---|
| Top-k precision | Fraction of manually judged relevant records in top 10 and top 20 | Required for every new source |
| Useful-source recall | Whether the expanded run finds known important papers/repos/pages missed by baseline | Required for Stage A and Stage B |
| Citation whitelist pass rate | Percent of generated reports with zero external URL violations | Required for all stages |
| Paragraph citation coverage | Percent of report paragraphs with valid evidence IDs | Required for all stages |
| Source diversity score | Distinct source classes used by the evidence set and final citations | Required for all stages |
| Noise displacement rate | Number of old high-quality baseline results pushed out by low-quality new results | Required for ranking changes |
| Failure visibility | Percent of failed source calls represented as structured `QueryAttempt` errors | Required for all adapters |

Suggested minimum gates before default enablement:

- top-10 precision should not drop by more than 5 percentage points against GitHub/arXiv baseline on existing benchmark topics;
- citation whitelist pass rate must remain 100%;
- paragraph citation coverage should remain at or above the current accepted threshold;
- no snippets-only web result may be cited;
- every source adapter must have empty-result and error-result fixtures.

## 3. Benchmark Topic Sets

Use small, reviewable benchmark sets before broad automation.

### Existing Technical Topics

These protect current GitHub/arXiv behavior:

- Rust agent framework
- LLM tool calling
- code agent benchmark
- controllable TTS
- retrieval augmented generation

Expected behavior: adding academic sources should enrich papers and metadata without burying relevant repositories.

### Academic Expansion Topics

These validate Stage A:

- large language model agent evaluation
- retrieval augmented generation benchmark
- speech synthesis controllability
- program repair benchmark
- multimodal instruction tuning

Expected behavior: Semantic Scholar/DBLP/OpenAlex/Crossref should find relevant non-arXiv or bibliographic records that arXiv alone misses.

### Domain Source Topics

These validate Stage B:

- neural machine translation evaluation
- clinical language model safety
- biomedical relation extraction
- chemical reaction prediction

Expected behavior: domain sources should help only when selected or clearly relevant; they should not affect default CS-only topics.

### Web Auxiliary Topics

These validate Stage C:

- latest LangGraph release notes
- OpenAI API model migration guide
- vLLM benchmark documentation
- PyTorch 2026 release changes

Expected behavior: web results should fetch official pages or documentation, not cite search snippets.

## 4. Fixture Test Requirements

Each source adapter should include fixtures before implementation is considered complete:

| Fixture | Required Count | Purpose |
|---|---:|---|
| Normal response | at least 2 | Ensure parser handles realistic records |
| Empty response | at least 1 | Ensure no false failure or fake fallback |
| Error response | at least 1 | Ensure structured `QueryAttempt.error` |
| Partial/missing fields | at least 1 | Ensure robust optional-field handling |
| Duplicate record | at least 1 per related source pair | Ensure dedup and merge rules work |

Parser tests must verify:

- stable IDs;
- title cleanup;
- author extraction where available;
- URL selection;
- date parsing;
- source-native metadata preservation;
- no panic on missing optional fields.

## 5. Ranking Regression Protocol

Ranking must be tested as a multi-lane problem, not one global flat list.

Recommended process:

1. Run baseline GitHub/arXiv on benchmark topics and store ranked source IDs.
2. Run expanded source set with the same budgets.
3. Compare:
   - baseline top-10 retained count;
   - new relevant discoveries;
   - low-quality displacement cases;
   - per-source distribution in the final top 20.
4. Flag regressions where:
   - a broad bibliographic source contributes more than half of top 20 by default;
   - web evidence appears above structured academic/repository evidence without explicit web-intent;
   - a source with abstracts missing dominates a chapter that requires paper evidence.

The ranking implementation should eventually support lane-guaranteed pools:

```text
per-source retrieval
  -> per-source top-k
  -> source-tier scoring
  -> duplicate merge
  -> global candidate pool
  -> optional rerank
  -> evidence memory
```

This follows the useful lesson from `daily-paper-reader`: global rerank works better when each query/source lane has guaranteed representation before final scoring.

## 6. Manual Precision Review

Before enabling a new source by default, run manual review on top-20 results for at least five topics.

Reviewer labels:

- `relevant`: directly useful for the research topic.
- `adjacent`: related but not strong enough for report evidence.
- `irrelevant`: off-topic.
- `duplicate`: already represented by a better source item.
- `metadata_only`: useful for validation but not enough for synthesis.
- `unsafe_to_cite`: snippet-only, missing landing page, or source quality too weak.

Manual review output should include:

- topic;
- source;
- rank;
- title;
- URL;
- label;
- reason;
- recommended ranking or adapter fix.

Pass condition:

- Stage A sources should achieve at least 70% `relevant` or `metadata_only` in top 20 for academic expansion topics.
- Stage B sources should achieve at least 70% `relevant` in top 20 only for their selected domains.
- Stage C web source should achieve at least 80% official/primary pages in top 10 for web-intent topics before citation is allowed.

## 7. Citation and Report Audit Protocol

Every generated report from an expanded source run must pass these checks:

- No generated URL outside `CitationLedger`.
- Every cited evidence ID exists in `EvidenceMemory`.
- Every cited URL has a source kind and source tier.
- Web citations have fetched content, not search snippets only.
- News/web citations include a publication or retrieval date when available.
- Claims about one source's metadata do not cite a different source's duplicate record unless merge provenance is explicit.

Cross-source verification gate:

- Key factual claims should be supported by more than one source when the claim is central to the chapter conclusion.
- Any key factual claim supported only by a single Tier 3 or Tier 4 source must be marked as `verification_gap`.
- If such a single-source Tier 3/4 claim remains in the report, the paragraph should explicitly label it as single-source and pending corroboration.
- A single Tier 1 or Tier 2 source can support source-native facts, such as a paper's title, repository stars, DOI metadata, venue, publication year, or official release date, but broader interpretive claims still need corroboration.
- Snippet-only discovery results cannot satisfy this gate.

For source diversity:

- The auditor should count source classes, not only enum variants.
- The minimum expectation for broad technical reports is two source classes when evidence exists.
- For implementation-heavy topics, GitHub or another artifact source should appear.
- For paper-heavy topics, at least one academic/preprint/venue source should appear.

## 8. Coverage Evaluation Protocol

Coverage should become source-aware.

Chapter evidence requirements should specify acceptable source classes:

| Chapter Intent | Acceptable Evidence |
|---|---|
| open-source implementation | GitHub, Hugging Face, official project page |
| academic methods | arXiv, Semantic Scholar, DBLP, OpenAlex, Crossref, ACL, OpenReview |
| recent product/version changes | official docs, release notes, fetched web pages, optionally news |
| cross-domain literature | domain preprint sources plus academic indexes |
| benchmark or dataset | paper source plus official benchmark/dataset page when available |

Coverage gap types should distinguish:

- `query_gap`: the source was queried but returned too little evidence.
- `source_gap`: the selected source is not suitable or failed.
- `class_gap`: the chapter has evidence, but from the wrong source class.
- `verification_gap`: a fact appears in only one weak source and needs corroboration.

## 9. Source Failure Protocol

Every adapter should produce structured diagnostics:

- source name;
- query text;
- started/finished time;
- result count;
- HTTP status when applicable;
- rate limit or retry-after metadata when applicable;
- parser error category;
- whether fallback was used;
- whether returned records are citeable or discovery-only.

Failure handling policy:

- A source failure should not fail the whole run if other source classes produced evidence.
- A failed required source class should produce a coverage warning.
- A repeated rate limit should recommend lowering budget or enabling cache.
- A parser failure should block that source's evidence rather than returning partially untrusted records as normal.

## 10. Rollout Gates

### Gate 1: Adapter-Level Readiness

- Fixture tests pass.
- Empty/error responses produce structured attempts.
- Stable IDs and URLs are deterministic.
- Native metadata is preserved.

### Gate 2: Evidence-Level Readiness

- Dedup works against arXiv/GitHub where applicable.
- Ranking regression does not bury baseline high-quality results.
- Manual top-20 precision passes source-specific threshold.

### Gate 3: Report-Level Readiness

- Citation whitelist pass rate remains 100%.
- Paragraph citation coverage remains acceptable.
- Source diversity improves or stays neutral.
- Writer text labels source types correctly.

### Gate 4: Default-Enablement Readiness

- Source passes at least five benchmark topics.
- Rate limit behavior is documented.
- User-facing configuration explains what enabling the source changes.
- Rollback is simple: disabling the source returns to GitHub/arXiv behavior without data migration.

## 11. Suggested Evaluation Artifacts

When implementation starts, create these artifacts under `eval/`:

```text
eval/fixtures/source_adapters/
  semantic_scholar_normal.json
  semantic_scholar_empty.json
  semantic_scholar_error.json
  dblp_normal.json
  dblp_empty.json
  dblp_error.json

eval/topics/search_boundary_topics.json
eval/expected/search_boundary_baseline.json
eval/results/search_boundary_regression.json
eval/reviews/manual_precision_template.csv
```

The existing `scripts/stage3_eval.mjs` validates control-loop shape. A future search-boundary eval should validate source quality, ranking stability, and citation integrity.

## 12. References

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
