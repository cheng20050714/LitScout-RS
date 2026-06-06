use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::LlmConfig;
use crate::error::{AppError, Result};
use crate::llm::deepseek::{
    ChatCompletionsRequest, ChatMessage, DeepSeekClient, DeepSeekConfig, ResponseFormat,
};
use crate::model::{ChapterNode, QueryPortfolio, ResearchBrief};
use crate::run_policy::RunPolicy;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChapterPlanOutput {
    pub chapters: Vec<ChapterNode>,
    pub query_portfolio: Vec<QueryPortfolio>,
    pub warnings: Vec<String>,
}

pub async fn generate_chapter_plan(
    brief: &ResearchBrief,
    policy: &RunPolicy,
    llm_config: &LlmConfig,
) -> Result<ChapterPlanOutput> {
    let plan = generate_chinese_plan(
        &brief.topic,
        policy.github_budget,
        policy.arxiv_budget,
        llm_config,
    )
    .await?;
    Ok(chapter_plan_from_plan_output(&plan, policy))
}

pub fn chapter_plan_from_plan_output(plan: &PlanOutput, policy: &RunPolicy) -> ChapterPlanOutput {
    let aspects = plan
        .aspects
        .iter()
        .take(policy.max_aspects_per_round.max(1))
        .collect::<Vec<_>>();
    let mut warnings = plan.warnings.clone();
    if aspects.is_empty() {
        warnings.push("搜索计划为空，使用默认章节。".to_string());
    }

    let fallback_aspect = AspectOutput {
        name_zh: "默认研究方向".to_string(),
        rationale_zh: "围绕原始主题进行 GitHub 与 arXiv 检索。".to_string(),
        github_query: plan.original_topic.clone(),
        arxiv_query: plan.original_topic.clone(),
        github_limit: policy.github_budget,
        arxiv_limit: policy.arxiv_budget,
    };
    let effective_aspects = if aspects.is_empty() {
        vec![&fallback_aspect]
    } else {
        aspects
    };

    let chapters = effective_aspects
        .iter()
        .enumerate()
        .map(|(index, aspect)| ChapterNode {
            id: format!("ch-{}", index + 1),
            parent_id: None,
            title_zh: aspect.name_zh.clone(),
            research_question: aspect.rationale_zh.clone(),
            required_evidence_kinds: vec!["github".to_string(), "arxiv".to_string()],
            evidence_quota: (aspect.github_limit + aspect.arxiv_limit).max(1),
            sort_order: index + 1,
        })
        .collect::<Vec<_>>();

    let query_portfolio = effective_aspects
        .iter()
        .enumerate()
        .map(|(index, aspect)| QueryPortfolio {
            chapter_id: format!("ch-{}", index + 1),
            github_queries: vec![aspect.github_query.clone()],
            arxiv_queries: vec![aspect.arxiv_query.clone()],
            rationale: aspect.rationale_zh.clone(),
            budget: (aspect.github_limit + aspect.arxiv_limit).max(1),
        })
        .collect();

    ChapterPlanOutput {
        chapters,
        query_portfolio,
        warnings,
    }
}

pub async fn generate_chinese_plan(
    topic: &str,
    github_limit: usize,
    arxiv_limit: usize,
    llm_config: &LlmConfig,
) -> Result<PlanOutput> {
    if !llm_config.enabled {
        return Ok(default_plan(topic, github_limit, arxiv_limit));
    }

    let config = DeepSeekConfig::from_llm_config(llm_config).ok_or_else(|| {
        AppError::InvalidConfig(
            "`--llm` requires a DeepSeek API key. Set DEEPSEEK_API_KEY or pass --deepseek-api-key."
                .to_string(),
        )
    })?;
    let client = DeepSeekClient::new(config)?;
    let request = build_plan_request(
        client.side_model(),
        format!(
            "请为以下技术主题生成搜索计划。\n主题：{topic}\nGitHub limit: {github_limit}\narXiv limit: {arxiv_limit}"
        ),
    );
    let response = client.chat_completions_with_retry(request).await?;
    let content = response.first_content()?;
    parse_plan_json(content, topic, github_limit, arxiv_limit)
}

pub fn default_plan(topic: &str, github_limit: usize, arxiv_limit: usize) -> PlanOutput {
    PlanOutput {
        plan_id: Uuid::new_v4().to_string(),
        original_topic: topic.to_string(),
        aspects: vec![AspectOutput {
            name_zh: "默认搜索".to_string(),
            rationale_zh: "未启用 LLM，使用原始主题作为搜索词。".to_string(),
            github_query: topic.to_string(),
            arxiv_query: topic.to_string(),
            github_limit,
            arxiv_limit,
        }],
        llm_generated: false,
        warnings: vec![
            "LLM 未启用，使用默认搜索计划；如需中文智能规划，请以 --llm 启动并配置 DeepSeek API key。"
                .to_string(),
        ],
    }
}

fn build_plan_request(model: String, content: String) -> ChatCompletionsRequest {
    ChatCompletionsRequest {
        model,
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: SEARCH_PLAN_SYSTEM_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content,
            },
        ],
        temperature: Some(0.2),
        max_tokens: Some(1024),
        stream: Some(false),
        response_format: Some(ResponseFormat {
            r#type: "json_object".to_string(),
        }),
    }
}

pub(crate) fn parse_plan_json(
    content: &str,
    topic: &str,
    github_limit: usize,
    arxiv_limit: usize,
) -> Result<PlanOutput> {
    let json = crate::llm::deepseek::strip_json_fence(content);
    let llm_response: LlmPlanResponse = serde_json::from_str(json)
        .map_err(|err| AppError::Llm(format!("章节计划 JSON 解析失败: {err}")))?;

    let raw_aspects = llm_response
        .aspects
        .into_iter()
        .take(3)
        .filter(|aspect| {
            !aspect.github_query.trim().is_empty() && !aspect.arxiv_query.trim().is_empty()
        })
        .collect::<Vec<_>>();

    if raw_aspects.is_empty() {
        return Err(AppError::Llm("章节计划未包含可用搜索方向".to_string()));
    }

    let github_per_aspect = split_limit(github_limit, raw_aspects.len());
    let arxiv_per_aspect = split_limit(arxiv_limit, raw_aspects.len());
    let aspects = raw_aspects
        .into_iter()
        .map(|aspect| AspectOutput {
            name_zh: non_empty_or(aspect.name_zh, "研究方向"),
            rationale_zh: non_empty_or(aspect.rationale_zh, "基于主题关键词的搜索方向。"),
            github_query: aspect.github_query.trim().to_string(),
            arxiv_query: aspect.arxiv_query.trim().to_string(),
            github_limit: github_per_aspect,
            arxiv_limit: arxiv_per_aspect,
        })
        .collect();

    Ok(PlanOutput {
        plan_id: Uuid::new_v4().to_string(),
        original_topic: topic.to_string(),
        aspects,
        llm_generated: true,
        warnings: Vec::new(),
    })
}

fn split_limit(limit: usize, parts: usize) -> usize {
    if parts == 0 {
        return limit.max(1);
    }
    limit.div_ceil(parts).max(1)
}

fn non_empty_or(value: String, default: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanOutput {
    pub plan_id: String,
    pub original_topic: String,
    pub aspects: Vec<AspectOutput>,
    pub llm_generated: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AspectOutput {
    pub name_zh: String,
    pub rationale_zh: String,
    pub github_query: String,
    pub arxiv_query: String,
    pub github_limit: usize,
    pub arxiv_limit: usize,
}

#[derive(Debug, Deserialize)]
struct LlmPlanResponse {
    aspects: Vec<LlmAspect>,
}

#[derive(Debug, Deserialize)]
struct LlmAspect {
    name_zh: String,
    rationale_zh: String,
    github_query: String,
    arxiv_query: String,
}

const SEARCH_PLAN_SYSTEM_PROMPT: &str = "\
你是 LitScout-RS 的研究侦察规划器。用户用中文描述技术主题，你生成受控搜索计划。

规则：
1. 生成 1-3 个搜索角度。
2. name_zh 和 rationale_zh 必须使用中文。
3. github_query 和 arxiv_query 必须使用英文技术术语或中英混合术语，以保证 GitHub/arXiv 检索效果。
4. 不要使用任意网页、浏览器、PDF 或外部搜索源。
5. 不要使用 AND/OR/NOT 等复杂布尔表达式，使用自然关键词即可。
6. 只返回 JSON，不要任何额外文本。

返回格式：
{
  \"aspects\": [
    {
      \"name_zh\": \"核心框架方向\",
      \"rationale_zh\": \"优先定位可直接复用的实现框架。\",
      \"github_query\": \"llm agent framework tool calling\",
      \"arxiv_query\": \"large language model tool calling agent\"
    }
  ]
}";

#[cfg(test)]
mod tests {
    use super::{default_plan, parse_plan_json};

    #[test]
    fn default_plan_is_valid_without_llm() {
        let plan = default_plan("Rust Agent 框架", 8, 6);

        assert!(!plan.llm_generated);
        assert_eq!(plan.original_topic, "Rust Agent 框架");
        assert_eq!(plan.aspects.len(), 1);
        assert_eq!(plan.aspects[0].github_limit, 8);
        assert_eq!(plan.aspects[0].arxiv_limit, 6);
        assert!(!plan.plan_id.is_empty());
    }

    #[test]
    fn parse_chinese_plan_json_from_fixture() {
        let content = r#"{
            "aspects": [
                {
                    "name_zh": "核心框架方向",
                    "rationale_zh": "寻找可复用 Rust agent 框架。",
                    "github_query": "rust agent framework tool calling",
                    "arxiv_query": "large language model agent framework"
                },
                {
                    "name_zh": "评测基准方向",
                    "rationale_zh": "补充 agent benchmark 相关论文和项目。",
                    "github_query": "agent benchmark rust",
                    "arxiv_query": "agent benchmark tool use"
                }
            ]
        }"#;

        let plan = parse_plan_json(content, "Rust Agent 框架", 9, 7).expect("plan should parse");

        assert!(plan.llm_generated);
        assert_eq!(plan.aspects.len(), 2);
        assert_eq!(plan.aspects[0].name_zh, "核心框架方向");
        assert_eq!(plan.aspects[0].github_limit, 5);
        assert_eq!(plan.aspects[0].arxiv_limit, 4);
        assert_eq!(plan.aspects[1].arxiv_query, "agent benchmark tool use");
    }
}
