export interface FrontendConfig {
  deepseek_api_key?: string;
  deepseek_base_url?: string;
  deepseek_model?: string;
  deepseek_side_model?: string;
  github_token?: string;
}

export type ResearchRunState =
  | "created"
  | "plan_ready"
  | "fetching"
  | "evidence_ready"
  | "synthesis_ready"
  | "completed"
  | "failed";

export interface RunPolicy {
  max_research_rounds: number;
  max_aspects_per_round: number;
  github_budget: number;
  arxiv_budget: number;
  auto_approve_plan: boolean;
  allow_github_enrich: boolean;
  require_citation_audit: boolean;
  skip_plan_critic: boolean;
  skip_coverage_critic: boolean;
  max_llm_calls_per_run: number;
}

export interface ResearchBrief {
  topic: string;
  user_intent: string;
  target_audience: string;
  time_scope: string;
  inclusion_criteria: string[];
  exclusion_criteria: string[];
  success_criteria: string[];
}

export interface ChapterNode {
  id: string;
  parent_id?: string | null;
  title_zh: string;
  research_question: string;
  required_evidence_kinds: string[];
  evidence_quota: number;
  sort_order: number;
}

export interface QueryPortfolio {
  chapter_id: string;
  github_queries: string[];
  arxiv_queries: string[];
  rationale: string;
  budget: number;
}

export interface QueryAttempt {
  query_id: string;
  source: "github" | "arxiv" | string;
  query: string;
  chapter_id: string;
  round: number;
  started_at: string;
  finished_at?: string | null;
  result_count: number;
  error?: string | null;
}

export interface EvidenceItem {
  evidence_id: string;
  source_item_id: string;
  citation_id: string;
  chapter_ids: string[];
  query_attempt_ids: string[];
  source_kind: "github" | "arxiv" | "GitHub" | "Arxiv";
  title: string;
  url: string;
  evidence_note_zh: string;
  evidence_snippet: string;
  support_score?: number | null;
}

export interface EvidenceMemory {
  items: EvidenceItem[];
  query_attempts: QueryAttempt[];
  source_lineage: SourceQueryLineage[];
}

export interface SourceQueryLineage {
  source_item_id: string;
  query_attempt_ids: string[];
}

export type GapKind = "query_gap" | "source_gap";
export type CoverageRecommendation = "no_action" | "suggest_new_query" | "out_of_scope";

export interface CoverageGap {
  chapter_id: string;
  gap_kind: GapKind;
  explanation: string;
  recommended_queries: string[];
  severity: "high" | "medium" | "low" | string;
}

export interface CoverageReport {
  gaps: CoverageGap[];
  out_of_scope_notice: string[];
  overall_coverage_score: number;
  recommendation: CoverageRecommendation;
}

export interface ParagraphWithCitations {
  text_zh: string;
  cited_evidence_ids: string[];
}

export interface ChapterDraft {
  chapter_id: string;
  title_zh: string;
  paragraphs: ParagraphWithCitations[];
}

export interface ReportDraft {
  title_zh: string;
  chapters: ChapterDraft[];
  global_summary_zh: string;
  written_at: string;
}

export interface CitationAuditReport {
  url_whitelist_passed: boolean;
  citation_coverage_ratio: number;
  source_diversity_score: number;
  freshness_warnings: string[];
  unsupported_paragraph_warnings: string[];
  external_url_violations: string[];
}

export interface ResearchRunRecord {
  run_id: string;
  topic: string;
  state: ResearchRunState;
  created_at: string;
  updated_at: string;
  policy: RunPolicy;
  brief?: ResearchBrief | null;
  chapters: ChapterNode[];
  query_portfolio: QueryPortfolio[];
  plan_warnings: string[];
  evidence_memory?: EvidenceMemory | null;
  coverage_report?: CoverageReport | null;
  report_draft?: ReportDraft | null;
  citation_audit?: CitationAuditReport | null;
  report_markdown?: string | null;
  output_report?: string | null;
  warnings: string[];
  origin_run_id?: string | null;
  origin_checkpoint_id?: string | null;
}

export interface StatefulRunResponse {
  run_id: string;
  topic: string;
  state: ResearchRunState;
  run: ResearchRunRecord;
}

export interface StatefulRunEvent {
  event: string;
  data?: {
    state?: ResearchRunState;
    checkpoint_id?: string;
    total?: number;
    gaps?: number;
    citation_coverage_ratio?: number;
    run_id?: string;
    error?: string;
  };
}

export interface StatefulRunStreamEvent {
  event: "agent" | "run_ready" | "run_failed";
  data: StatefulRunEvent | StatefulRunResponse | { error: string };
}

export interface Checkpoint {
  checkpoint_id: string;
  run_id: string;
  state: ResearchRunState;
  snapshot_path: string;
  created_at: string;
  rollback_allowed: boolean;
}

export interface CheckpointListResponse {
  run_id: string;
  checkpoints: Checkpoint[];
}

export interface EvidenceResponse {
  run_id: string;
  evidence_memory: EvidenceMemory;
}

export interface CoverageResponse {
  run_id: string;
  coverage_report: CoverageReport;
}

export interface CitationAuditResponse {
  run_id: string;
  citation_audit: CitationAuditReport;
}

export interface StatefulFollowupResponse {
  run_id: string;
  route:
    | { route: "answer"; answer: string }
    | { route: "incremental_research_required"; reason: string }
    | Record<string, unknown>;
}

export interface ReportChatResponse {
  answer: string;
}

export interface ChatStreamEvent {
  event: "delta" | "done" | "failed";
  data: {
    text?: string;
    error?: string;
  };
}

export interface ReportTranslateResponse {
  translated_markdown: string;
}

export interface HealthResponse {
  status: string;
  service: string;
  llm_enabled: boolean;
  github_token_configured: boolean;
}
