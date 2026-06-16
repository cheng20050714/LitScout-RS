use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunPolicy {
    pub max_research_rounds: usize,
    pub max_aspects_per_round: usize,
    pub github_budget: usize,
    pub arxiv_budget: usize,
    #[serde(default)]
    pub academic_extra_enabled: bool,
    #[serde(default = "default_academic_budget")]
    pub academic_budget: usize,
    pub auto_approve_plan: bool,
    pub allow_github_enrich: bool,
    pub require_citation_audit: bool,
    pub skip_plan_critic: bool,
    pub skip_coverage_critic: bool,
    pub max_llm_calls_per_run: usize,
}

impl Default for RunPolicy {
    fn default() -> Self {
        Self {
            max_research_rounds: 1,
            max_aspects_per_round: 3,
            github_budget: 10,
            arxiv_budget: 10,
            academic_extra_enabled: false,
            academic_budget: default_academic_budget(),
            auto_approve_plan: false,
            allow_github_enrich: true,
            require_citation_audit: true,
            skip_plan_critic: false,
            skip_coverage_critic: false,
            max_llm_calls_per_run: 10,
        }
    }
}

impl RunPolicy {
    pub fn bounded(mut self) -> Self {
        self.max_research_rounds = self.max_research_rounds.clamp(1, 1);
        self.max_aspects_per_round = self.max_aspects_per_round.clamp(1, 3);
        self.github_budget = self.github_budget.clamp(1, 50);
        self.arxiv_budget = self.arxiv_budget.clamp(1, 50);
        self.academic_budget = self.academic_budget.clamp(1, 50);
        self.max_llm_calls_per_run = self.max_llm_calls_per_run.clamp(1, 20);
        self
    }
}

fn default_academic_budget() -> usize {
    10
}

#[cfg(test)]
mod tests {
    use super::RunPolicy;

    #[test]
    fn run_policy_bounds_values() {
        let policy = RunPolicy {
            max_research_rounds: 5,
            max_aspects_per_round: 99,
            github_budget: 0,
            arxiv_budget: 100,
            academic_budget: 0,
            max_llm_calls_per_run: 0,
            ..RunPolicy::default()
        }
        .bounded();

        assert_eq!(policy.max_research_rounds, 1);
        assert_eq!(policy.max_aspects_per_round, 3);
        assert_eq!(policy.github_budget, 1);
        assert_eq!(policy.arxiv_budget, 50);
        assert_eq!(policy.academic_budget, 1);
        assert_eq!(policy.max_llm_calls_per_run, 1);
    }

    #[test]
    fn run_policy_deserializes_without_academic_fields() {
        let json = r#"{
            "max_research_rounds": 1,
            "max_aspects_per_round": 3,
            "github_budget": 10,
            "arxiv_budget": 10,
            "auto_approve_plan": false,
            "allow_github_enrich": true,
            "require_citation_audit": true,
            "skip_plan_critic": false,
            "skip_coverage_critic": false,
            "max_llm_calls_per_run": 10
        }"#;

        let policy: RunPolicy = serde_json::from_str(json).expect("legacy policy parses");

        assert!(!policy.academic_extra_enabled);
        assert_eq!(policy.academic_budget, 10);
    }
}
