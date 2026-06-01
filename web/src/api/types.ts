export interface AspectDto {
  name_zh: string;
  rationale_zh: string;
  github_query: string;
  arxiv_query: string;
  github_limit: number;
  arxiv_limit: number;
}

export interface PlanResponse {
  plan_id: string;
  original_topic: string;
  language: string;
  aspects: AspectDto[];
  llm_generated: boolean;
  warnings: string[];
}

export interface PlanRequest {
  topic: string;
  github_limit: number;
  arxiv_limit: number;
  language: string;
  config: FrontendConfig;
}

export interface FrontendConfig {
  deepseek_api_key?: string;
  deepseek_base_url?: string;
  deepseek_model?: string;
  deepseek_side_model?: string;
  github_token?: string;
}

export interface RunResponse {
  session_id: string;
  output_report: string;
  session_path?: string;
  report_markdown: string;
  warnings: string[];
  citations: Citation[];
  ranked_items: SourceItem[];
  quality: QualityReport;
}

export interface ReportChatResponse {
  answer: string;
}

export interface RunEvent {
  event: string;
  data:
    | {
        event: string;
        data?: {
          source?: string;
          count?: number;
          total?: number;
          message?: string;
          output_report?: string;
          session_path?: string;
        };
      }
    | RunResponse
    | {
        error: string;
      }
    | {
        source?: string;
        count?: number;
        total?: number;
        message?: string;
        session_id?: string;
        error?: string;
      };
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

export interface QualityReport {
  passed: boolean;
  warnings: string[];
  llm_repaired: boolean;
}

export interface SourceItem {
  id: string;
  kind: "github" | "arxiv" | "GitHub" | "Arxiv";
  title: string;
  url: string;
  summary: string;
  evidence_snippet: string;
  tags: string[];
  score: number;
  score_reasons: string[];
  classification_reasons: string[];
  published_or_updated_at?: string;
}

export interface LegacyRunEvent {
  event: string;
  data: {
    source?: string;
    count?: number;
    total?: number;
    message?: string;
    session_id?: string;
    error?: string;
  };
}

export interface HealthResponse {
  status: string;
  service: string;
  llm_enabled: boolean;
  github_token_configured: boolean;
}

export interface Citation {
  id: string;
  title: string;
  url: string;
  source_kind: "github" | "arxiv" | "GitHub" | "Arxiv";
  snippet: string;
}
