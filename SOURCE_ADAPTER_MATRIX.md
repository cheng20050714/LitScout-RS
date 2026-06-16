# Source Adapter Matrix

Planning date: 2026-06-15

This matrix records candidate sources for expanding LitScout-RS beyond GitHub and arXiv. It is meant to drive implementation decisions later, not to change runtime behavior now.

## 1. Admission Rubric

Each source is evaluated on five dimensions:

- **Authority**: how trustworthy the source is for technical research.
- **Structure**: whether records include machine-readable title, authors, date, venue, URL, abstract, DOI, citation counts, or repository metadata.
- **Precision risk**: likelihood that naive query matching returns off-topic records.
- **Operational risk**: rate limits, auth requirements, anti-bot behavior, parser fragility, or maintenance cost.
- **LitScout role**: whether the source should enter the core evidence pool, a domain-specific pool, or only an auxiliary web lane.

Recommended status values:

- `v1-core`: implement in the first code expansion wave.
- `v1-optional`: implement early, but keep behind explicit enablement.
- `later`: useful, but not first-wave.
- `defer`: document only; high risk or outside current product boundary.

## 2. Adapter Matrix

| Source | Class | API / Access | Useful Fields | Accuracy Value | Main Failure Modes | Operational Notes | Status |
|---|---|---|---|---|---|---|---|
| GitHub | Implementation | GitHub repository search API; optional README fetch | repo name, description, stars, forks, language, topics, update time, README excerpt | Existing implementation evidence; best for code availability and ecosystem maturity | search qualifiers in raw topic, rate limits, README N+1 requests | Keep as default core source; token recommended | existing core |
| arXiv | Preprint | arXiv Atom API | arXiv ID, title, authors, abstract, categories, published/updated dates, PDF URL | Existing preprint source; strong for AI/CS early work | rate limits, category noise, arXiv-only blind spots | Keep as default core source and full-text bridge | existing core |
| Semantic Scholar | Academic index | Academic Graph paper search API | paper ID, title, authors, abstract, year, venue, citation count, URL, fields of study | High recall and ranking signal through citations | broad matching can return adjacent but irrelevant work; rate limits | First new academic adapter; no browser needed | v1-core |
| DBLP | CS bibliography | `https://dblp.org/search/publ/api` | title, authors, venue, year, DOI/ee URL, DBLP key | High-precision CS venue metadata; good duplicate verifier | no abstracts; weak semantic relevance; client-side year filtering needed | Pair with arXiv/Semantic Scholar, not standalone synthesis evidence | v1-core |
| OpenAlex | Open scholarly graph | OpenAlex Works API | DOI, title, authorships, institutions, source, publication year, concepts/topics, cited-by count, OA status | Broad open metadata and DOI/institution normalization | broad coverage can add noisy non-CS records; requires source tiering | Free API key expected by current docs; useful for metadata merge | v1-core |
| Crossref | DOI metadata | Crossref REST `/works` endpoint | DOI, title, container title, publisher, issued date, type, reference/license metadata | DOI validation and metadata fallback | abstracts often missing; title search can be noisy | Better as validation/enrichment than primary retrieval | v1-optional |
| ACL Anthology | Venue corpus | Official GitHub XML data / Python API / static metadata | Anthology ID, title, authors, venue/booktitle, year, PDF URL | Excellent NLP/CL source; high precision for LLM/NLP topics | XML ingest and local index needed; not broad outside ACL venues | Prefer local indexed metadata over live scraping | later |
| OpenReview | Conference platform | OpenReview API V2 | forum/note IDs, title, authors, abstract, venue invitation, decision, reviews where public | Strong for ICLR/NeurIPS/ICML and review-aware scouting | venue-specific invitations; schema changes; withdrawn/revised papers | Enable by venue profile; avoid mixing reviews into citation claims without labels | later |
| bioRxiv | Domain preprint | bioRxiv API | DOI, title, authors, abstract, date, category, server, version | Useful for biology-adjacent topics | poor fit for default CS topics; can hurt precision | Explicit domain opt-in only | later |
| medRxiv | Domain preprint | medRxiv API, same family as bioRxiv | DOI, title, authors, abstract, date, category, server, version | Useful for medical topics | high-stakes interpretation risk; not default technical scouting | Explicit domain opt-in; report should label medical preprint status | later |
| ChemRxiv | Domain preprint | ChemRxiv API or metadata endpoints, depending on final implementation research | DOI, title, authors, abstract, date, category | Useful for chemistry topics | less relevant to current user base; API shape needs confirmation before coding | Do not include until a chemistry use case exists | later |
| Hugging Face | Model/dataset implementation | Hugging Face Hub API | model/dataset name, downloads, likes, tags, pipeline task, update time, card text | Useful for ML systems where artifacts are models/datasets rather than repos | popularity metrics can bias toward generic models; cards vary in quality | Good future implementation-artifact source after academic Stage A | later |
| Serper / Google Search | Web discovery | Serper Google search API | title, URL, snippet, domain | Finds official pages, docs, release notes, benchmarks | snippets are not evidence; API key and quota; search ranking bias | Web lane only; must fetch page before citation | later |
| DuckDuckGo HTML | Web discovery fallback | HTML result page scraping | title, URL, snippet | No API key fallback for discovery | fragile selectors; anti-bot / layout change; lower reliability | Last-resort discovery only | later |
| Jina Reader | Web/page reading | `https://r.jina.ai/{url}` and reader headers | page title, markdown content, links, PDF/web text | Converts pages/PDFs into citeable text for web evidence | rate limits, extraction errors, missing dynamic content | Preferred web fetch backend; already used in reading full-text flow | later |
| Direct HTTP + local extraction | Web/page reading fallback | `reqwest` + HTML extraction library | raw HTML/text, title, extracted body | Removes dependency on one reader service | JS-heavy pages fail; boilerplate/noise; robots/legal concerns | Fallback only; include SSRF and search-page blocks | later |
| NewsAPI | News | `/v2/everything` | title, source, author, URL, description, publishedAt, truncated content | Useful for current events, company announcements, regulation | news is time-sensitive; relevance varies; API key needed | Not default for technical literature; use only for "latest/current" prompts | defer |
| Google News RSS | News fallback | RSS search feed | title, source, link, pubDate, description | No-key fallback for recent events | RSS parsing fragility; redirect URLs; source quality varies | Auxiliary only | defer |
| WeChat via Sogou | Chinese social/web | Sogou WeChat search + mp.weixin article fetch | title, proxied URL, article text, publish time | Useful for Chinese public-account content | anti-spider, captcha, fragile JS redirect parsing, unclear provenance quality | Keep out of Rust core for now | defer |
| Browser / CDP | Dynamic web | Playwright or Chrome DevTools Protocol | rendered DOM text, authenticated content if user profile used | Handles JS-heavy/auth-walled pages | high maintenance, privacy risk, anti-bot behavior, user setup burden | Do not add to default LitScout-RS core | defer |

## 3. Access, Auth, and Rate-Limit Specification

This section records the adapter integration contract, not permanent external quota guarantees. Exact provider limits and authentication rules must be rechecked against official docs during implementation and reflected in fixtures or config defaults.

| Source | Auth Requirement | Config Surface | Rate-Limit Handling | Recommended Run Budget |
|---|---|---|---|---|
| GitHub | Optional but recommended for stable quota | `GITHUB_TOKEN` or existing credential path | Track remaining quota and reset time when headers are available; back off on 403/429 | Keep as existing default budget; README fetches should remain separately capped |
| arXiv | None | none | Respect polite delay guidance; retry transient errors with bounded backoff | Keep existing default budget |
| Semantic Scholar | Optional API key for higher quota | `SEMANTIC_SCHOLAR_API_KEY` | Treat 429 as a structured `QueryAttempt` failure with retry-after/backoff metadata | First Stage A adapter; cap per run until ranking regression is stable |
| DBLP | None | none | Use conservative concurrency; retry 429/5xx with bounded backoff | Pair with another academic source rather than broad standalone expansion |
| OpenAlex | Public access; contact email or provider key may be recommended by current docs | `OPENALEX_EMAIL` or future provider-specific key | Preserve remaining/reset metadata when available; back off on 429 | Enable after dedup rules are stable |
| Crossref | Public access; contact email recommended for polite use | `CROSSREF_MAILTO` | Use polite user agent/contact metadata; back off on 429/503 | Use mainly for DOI validation and enrichment |
| ACL Anthology | None if using public static metadata | local metadata path or release URL | Prefer local indexed metadata over repeated live fetches | Domain/profile budget only |
| OpenReview | Usually public for public venues; credentials only for non-public data, which should stay out of core flow | none for public mode | Venue-scoped pagination and bounded retries | Domain/profile budget only |
| bioRxiv / medRxiv / ChemRxiv | Public access, subject to final API confirmation | source-specific endpoint config | Conservative pagination and bounded retry on 429/5xx | Explicit domain opt-in only |
| Serper / Google Search | Required paid/provider key | `SERPER_API_KEY` | Record quota failures as discovery failures; never convert snippets into evidence | Web-intent chapters only |
| DuckDuckGo HTML | None | none | Low concurrency; selectors treated as fragile parser fixtures | Last-resort discovery fallback only |
| Jina Reader | Public or key-based depending on deployment/provider policy | `JINA_API_KEY` if required | Record extraction failures separately from discovery failures | Fetch only URLs admitted by web lane |
| NewsAPI | Required provider key | `NEWSAPI_KEY` | Strict per-run cap because news is not a core source | Defer |

Adapters should expose rate-limit data through `QueryAttempt.rate_limit_info`. Missing quota headers should be represented as unknown rather than guessed.

## 4. Normalized Source Fields

Every adapter should map native records into a common evidence shape:

| Normalized Field | Required | Notes |
|---|---:|---|
| `source_item_id` | yes | Stable prefix plus native ID: `semantic_scholar:<paperId>`, `dblp:<key>`, `openalex:<work_id>`, `doi:<doi>` |
| `source_kind` | yes | Should distinguish implementation, preprint, academic index, venue corpus, domain preprint, web, news |
| `title` | yes | Cleaned, human-readable title |
| `url` | yes | Landing page preferred over API URL; PDF URL can be metadata |
| `summary` | yes | Abstract, description, README excerpt, or fetched page summary |
| `evidence_snippet` | yes | Short, citeable excerpt; snippets from search engines are discovery-only until fetched |
| `published_or_updated_at` | no | Use source-provided published/update date when available |
| `tags` | no | Categories, topics, fields of study, repo topics, venue tags |
| `metadata` | yes | Preserve native fields needed for ranking, display, and dedup |

Planned `SourceKind` expansion:

| SourceKind | Intended Sources | Notes |
|---|---|---|
| `GitHub` | GitHub repositories | Existing implementation source. |
| `Arxiv` | arXiv papers | Existing preprint source and reading-library bridge. |
| `AcademicIndex` | Semantic Scholar, OpenAlex | Broad academic indexes with abstracts, citations, concepts, or graph metadata. |
| `Bibliography` | DBLP, Crossref | Bibliographic/DOI metadata; often best for validation and dedup. |
| `VenueCorpus` | ACL Anthology | Curated venue corpus with stable paper IDs and PDFs. |
| `ConferencePlatform` | OpenReview | Conference platform records, decisions, and public review metadata where available. |
| `DomainPreprint` | bioRxiv, medRxiv, ChemRxiv | Domain-specific preprint servers; opt-in by topic/profile. |
| `ModelHub` | Hugging Face | Future artifact source for models and datasets. |
| `WebOfficial` | Official docs, release notes, project pages, benchmark pages | Auxiliary web evidence from fetched official pages. |
| `WebGeneral` | News, blogs, WeChat, general web pages | Context or recency support only; not a default core source. |

## 5. Query Attempt and Lineage Schema

Future adapters should emit structured query diagnostics rather than free-form logs. The current Rust structs can evolve toward this shape while preserving existing fields with serde defaults.

`QueryAttempt` fields:

| Field | Required | Meaning |
|---|---:|---|
| `attempt_id` | yes | Stable run-local attempt ID. Existing `query_id` can serve this role during migration. |
| `chapter_id` | yes | Chapter that requested the query. |
| `source_name` | yes | Adapter name such as `github`, `arxiv`, `semantic_scholar`, or `dblp`. |
| `source_kind` | yes | Normalized source kind used for ranking and coverage. |
| `query_text` | yes | Exact query sent to the source. |
| `started_at` | yes | Attempt start timestamp. |
| `finished_at` | no | Attempt finish timestamp if the call completed. |
| `result_count` | yes | Number of normalized source items admitted from this attempt. |
| `http_status` | no | HTTP status when applicable. |
| `rate_limit_info` | no | Retry-after, reset time, remaining quota, or source-specific limit note. |
| `parser_error` | no | Parser or normalization failure category. |
| `is_citeable` | yes | Whether returned items are citeable evidence or discovery-only hints. |
| `error` | no | Human-readable source failure summary. |

`SourceQueryLineage` fields:

| Field | Required | Meaning |
|---|---:|---|
| `lineage_id` | yes | Stable lineage record ID, usually derived from source item and attempt IDs. |
| `source_item_id` | yes | Normalized source item ID. |
| `chapter_id` | yes | Chapter where the source item was discovered or admitted. |
| `source_kind` | yes | Normalized source kind. |
| `query_attempt_ids` | yes | Attempts that returned or admitted this item. |
| `returned_item_ids` | yes | Source items returned by the attempt set; useful before dedup/merge. |
| `merged_from_item_ids` | no | Duplicate source items merged into this canonical item. |

The implementer should not let a source item bypass lineage. If a result cannot be linked to a query attempt, it should remain outside `EvidenceMemory` and outside `CitationLedger`.

## 6. Source Tier Policy

Ranking should treat source type as a guardrail, not only a bonus.

| Tier | Sources | Ranking Role |
|---|---|---|
| Tier 1 | GitHub, arXiv, Semantic Scholar, DBLP, OpenAlex, Crossref | Core evidence pool |
| Tier 2 | ACL Anthology, OpenReview, bioRxiv, medRxiv, ChemRxiv, Hugging Face | Domain/source-profile evidence pool |
| Tier 3 | Official docs, release notes, benchmark pages, project homepages fetched through web lane | Auxiliary evidence pool |
| Tier 4 | News, blogs, WeChat, general web pages | Context or recency support only |

Default reports should not allow Tier 3 or Tier 4 evidence to satisfy a chapter's academic/source quota unless the chapter explicitly asks for current web evidence.

## 7. Duplicate and Merge Rules

The expansion will increase duplicates. Dedup should be planned before adding broad sources.

Recommended stable merge priority:

1. DOI exact match.
2. arXiv ID exact match.
3. OpenReview forum/note ID exact match.
4. ACL Anthology ID exact match.
5. DBLP key exact match.
6. Normalized title + first author + year.

When duplicates are merged:

- keep all source provenance entries;
- choose the most citeable landing URL by priority: publisher/venue page, arXiv abs page, OpenReview forum, DOI URL, index page;
- keep the richest abstract/summary;
- keep source-specific ranking features such as citations, stars, venue, category, and update date.

## 8. Recommended First Implementation Slice

When code work begins, the first slice should be deliberately small:

1. Add `SemanticScholarAdapter`.
2. Add `DblpAdapter`.
3. Extend source kind/tier metadata.
4. Keep GitHub/arXiv default behavior unchanged unless `--academic-extra` or equivalent UI setting is enabled.
5. Add fixture parser tests and ranking regression before enabling in default runs.

OpenAlex and Crossref should follow once dedup and metadata merge rules are stable.

## 9. References

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
