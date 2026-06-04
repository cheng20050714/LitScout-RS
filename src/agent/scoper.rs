use crate::config::LlmConfig;
use crate::error::Result;
use crate::model::ResearchBrief;
use crate::run_policy::RunPolicy;

pub async fn generate_research_brief(
    topic: &str,
    _policy: &RunPolicy,
    _llm_config: &LlmConfig,
) -> Result<ResearchBrief> {
    Ok(ResearchBrief::from_topic(topic))
}

#[cfg(test)]
mod tests {
    use crate::config::LlmConfig;
    use crate::run_policy::RunPolicy;

    use super::generate_research_brief;

    #[tokio::test]
    async fn scoper_builds_brief_without_network() {
        let brief = generate_research_brief(
            "Rust Agent",
            &RunPolicy::default(),
            &LlmConfig::from_env(false, 30),
        )
        .await
        .expect("brief should build");

        assert_eq!(brief.topic, "Rust Agent");
        assert!(!brief.success_criteria.is_empty());
    }
}
