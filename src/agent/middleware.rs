use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::{AppError, Result};
use crate::trace::stable_hash;

#[derive(Debug, Clone)]
pub struct AgentContext {
    pub actor: String,
    pub input: String,
    pub allowed_urls: Vec<String>,
    pub max_llm_calls: usize,
}

pub trait AgentMiddleware: Send + Sync {
    fn before_llm_call(&self, ctx: &AgentContext) -> Result<()>;
    fn after_llm_call(&self, ctx: &AgentContext, response: &str) -> Result<()>;
}

#[derive(Default)]
pub struct CitationGuard;

impl AgentMiddleware for CitationGuard {
    fn before_llm_call(&self, _ctx: &AgentContext) -> Result<()> {
        Ok(())
    }

    fn after_llm_call(&self, ctx: &AgentContext, response: &str) -> Result<()> {
        if ctx.allowed_urls.is_empty() {
            return Ok(());
        }
        for url in extract_urls(response) {
            if !ctx.allowed_urls.iter().any(|allowed| allowed == &url) {
                return Err(AppError::Llm(format!(
                    "{} output referenced URL outside ledger: {url}",
                    ctx.actor
                )));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct JsonSchemaGuard;

impl AgentMiddleware for JsonSchemaGuard {
    fn before_llm_call(&self, _ctx: &AgentContext) -> Result<()> {
        Ok(())
    }

    fn after_llm_call(&self, ctx: &AgentContext, response: &str) -> Result<()> {
        serde_json::from_str::<serde_json::Value>(crate::llm::deepseek::strip_json_fence(response))
            .map(|_| ())
            .map_err(|err| AppError::Llm(format!("{} returned invalid JSON: {err}", ctx.actor)))
    }
}

pub struct TokenBudgetTracker {
    calls: AtomicUsize,
}

impl TokenBudgetTracker {
    pub fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Default for TokenBudgetTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentMiddleware for TokenBudgetTracker {
    fn before_llm_call(&self, ctx: &AgentContext) -> Result<()> {
        let next = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if next > ctx.max_llm_calls {
            return Err(AppError::Workflow(format!(
                "LLM call budget exceeded at actor `{}`; input hash {}",
                ctx.actor,
                stable_hash(&ctx.input)
            )));
        }
        Ok(())
    }

    fn after_llm_call(&self, _ctx: &AgentContext, _response: &str) -> Result<()> {
        Ok(())
    }
}

fn extract_urls(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|token| token.find("http").map(|start| token[start..].to_string()))
        .map(|url| {
            url.trim_matches(|ch: char| {
                matches!(
                    ch,
                    ')' | ']' | '}' | ',' | '.' | ';' | '。' | '，' | '；' | '）'
                )
            })
            .to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{AgentContext, AgentMiddleware, CitationGuard, TokenBudgetTracker};

    #[test]
    fn citation_guard_rejects_external_url() {
        let ctx = AgentContext {
            actor: "writer".to_string(),
            input: "input".to_string(),
            allowed_urls: vec!["https://github.com/acme/repo".to_string()],
            max_llm_calls: 2,
        };

        let err = CitationGuard
            .after_llm_call(&ctx, "see https://example.com")
            .expect_err("external URL should fail");

        assert!(err.to_string().contains("outside ledger"));
    }

    #[test]
    fn token_budget_tracker_enforces_limit() {
        let tracker = TokenBudgetTracker::new();
        let ctx = AgentContext {
            actor: "planner".to_string(),
            input: "input".to_string(),
            allowed_urls: Vec::new(),
            max_llm_calls: 1,
        };

        tracker.before_llm_call(&ctx).expect("first call allowed");
        assert!(tracker.before_llm_call(&ctx).is_err());
    }
}
