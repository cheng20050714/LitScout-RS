use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::workflow_state::ResearchRunState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum TraceEvent {
    StateTransition {
        from: ResearchRunState,
        to: ResearchRunState,
        at: DateTime<Utc>,
    },
    LlmRequestStarted {
        actor: String,
        model: String,
        input_hash: String,
        at: DateTime<Utc>,
    },
    LlmRequestFinished {
        actor: String,
        output_hash: String,
        token_estimate: usize,
        at: DateTime<Utc>,
    },
    AgentDecision {
        actor: String,
        input_hash: String,
        output_hash: String,
        rationale: String,
        at: DateTime<Utc>,
    },
    ToolCallStarted {
        tool: String,
        query: String,
        at: DateTime<Utc>,
    },
    ToolCallFinished {
        tool: String,
        result_count: usize,
        error: Option<String>,
        at: DateTime<Utc>,
    },
    QualityWarning {
        message: String,
        at: DateTime<Utc>,
    },
    CoverageGapDetected {
        chapter_id: String,
        gap_kind: String,
        at: DateTime<Utc>,
    },
    CheckpointCreated {
        checkpoint_id: String,
        state: ResearchRunState,
        at: DateTime<Utc>,
    },
    RollbackBranchCreated {
        origin_run_id: String,
        new_run_id: String,
        at: DateTime<Utc>,
    },
}

pub struct TraceWriter {
    path: PathBuf,
}

impl TraceWriter {
    pub async fn new(run_dir: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(run_dir).await?;
        Ok(Self {
            path: run_dir.join("trace.jsonl"),
        })
    }

    pub async fn append(&self, event: &TraceEvent) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        let line = serde_json::to_string(event)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }
}

pub fn stable_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let hash = hasher.finalize();
    hex::encode(hash)[..16].to_string()
}

pub fn token_estimate(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::{stable_hash, token_estimate};

    #[test]
    fn stable_hash_is_short_and_stable() {
        assert_eq!(stable_hash("abc"), stable_hash("abc"));
        assert_eq!(stable_hash("abc").len(), 16);
    }

    #[test]
    fn token_estimate_rounds_up() {
        assert_eq!(token_estimate("abcde"), 2);
    }
}
