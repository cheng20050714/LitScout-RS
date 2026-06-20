use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::workflow_state::{ResearchRunRecord, ResearchRunState};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkpoint {
    pub checkpoint_id: String,
    pub run_id: String,
    pub state: ResearchRunState,
    pub snapshot_path: String,
    pub created_at: DateTime<Utc>,
    pub rollback_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointSnapshot {
    pub checkpoint: Checkpoint,
    pub run: ResearchRunRecord,
}

pub async fn write_checkpoint(run_dir: &Path, run: &ResearchRunRecord) -> Result<Checkpoint> {
    let checkpoint_id = format!("cp-{}", Uuid::new_v4());
    let checkpoint_dir = run_dir.join("checkpoints");
    tokio::fs::create_dir_all(&checkpoint_dir).await?;
    let file_name = format!(
        "{}-{}.json",
        run.state_name(),
        checkpoint_id.trim_start_matches("cp-")
    );
    let snapshot_path = PathBuf::from("checkpoints").join(&file_name);
    let checkpoint = Checkpoint {
        checkpoint_id: checkpoint_id.clone(),
        run_id: run.run_id.clone(),
        state: run.state.clone(),
        snapshot_path: snapshot_path.display().to_string(),
        created_at: Utc::now(),
        rollback_allowed: matches!(run.state, ResearchRunState::PlanReady),
    };
    let snapshot = CheckpointSnapshot {
        checkpoint: checkpoint.clone(),
        run: run.clone(),
    };
    let body = serde_json::to_string_pretty(&snapshot)?;
    reject_secret_markers(&body)?;
    tokio::fs::write(checkpoint_dir.join(file_name), body).await?;
    Ok(checkpoint)
}

pub async fn read_checkpoint(
    run_dir: &Path,
    checkpoint: &Checkpoint,
) -> Result<CheckpointSnapshot> {
    let path = run_dir.join(&checkpoint.snapshot_path);
    let body = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&body)?)
}

fn reject_secret_markers(body: &str) -> Result<()> {
    for marker in [
        "DEEPSEEK_API_KEY",
        "GITHUB_TOKEN",
        "deepseek-secret",
        "github-secret",
    ] {
        if body.contains(marker) {
            return Err(AppError::Workflow(format!(
                "checkpoint snapshot contains forbidden secret marker `{marker}`"
            )));
        }
    }
    if contains_probable_secret_key(body) {
        return Err(AppError::Workflow(
            "checkpoint snapshot contains forbidden secret marker `sk-`".to_string(),
        ));
    }
    Ok(())
}

fn contains_probable_secret_key(body: &str) -> bool {
    const SECRET_PREFIX: &str = "sk-";
    const MIN_SECRET_SUFFIX_LEN: usize = 20;

    let mut search_from = 0usize;
    while let Some(relative_index) = body[search_from..].find(SECRET_PREFIX) {
        let start = search_from + relative_index;
        let prefix_boundary = start == 0
            || body[..start]
                .chars()
                .next_back()
                .is_none_or(|ch| !is_secret_token_char(ch));
        if !prefix_boundary {
            search_from = start + SECRET_PREFIX.len();
            continue;
        }

        let suffix_len = body[start + SECRET_PREFIX.len()..]
            .chars()
            .take_while(|ch| is_secret_token_char(*ch))
            .count();
        if suffix_len >= MIN_SECRET_SUFFIX_LEN {
            return true;
        }
        search_from = start + SECRET_PREFIX.len();
    }

    false
}

fn is_secret_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

trait StateName {
    fn state_name(&self) -> &'static str;
}

impl StateName for ResearchRunRecord {
    fn state_name(&self) -> &'static str {
        match self.state {
            ResearchRunState::Created => "created",
            ResearchRunState::PlanReady => "plan_ready",
            ResearchRunState::Fetching => "fetching",
            ResearchRunState::EvidenceReady => "evidence_ready",
            ResearchRunState::SynthesisReady => "synthesis_ready",
            ResearchRunState::Completed => "completed",
            ResearchRunState::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::run_policy::RunPolicy;
    use crate::workflow_state::{ResearchRunRecord, ResearchRunState};

    use super::write_checkpoint;

    #[tokio::test]
    async fn checkpoint_serialization_does_not_write_secret_markers() {
        let run_dir = temp_dir("checkpoint-no-secret");
        let mut run = ResearchRunRecord::new(
            "run-1".to_string(),
            "rust agent".to_string(),
            RunPolicy::default(),
        );
        run.state = ResearchRunState::PlanReady;
        let checkpoint = write_checkpoint(&run_dir, &run)
            .await
            .expect("checkpoint should write");
        let body = std::fs::read_to_string(run_dir.join(checkpoint.snapshot_path))
            .expect("checkpoint should be readable");

        assert!(!body.contains("deepseek-secret"));
        assert!(!body.contains("github-secret"));
        assert!(body.contains("run-1"));
    }

    #[tokio::test]
    async fn checkpoint_allows_benign_sk_substrings_in_evidence_text() {
        let run_dir = temp_dir("checkpoint-benign-sk");
        let mut run = ResearchRunRecord::new(
            "run-1".to_string(),
            "task-oriented Mask-GCT research".to_string(),
            RunPolicy::default(),
        );
        run.state = ResearchRunState::PlanReady;
        run.warnings
            .push("task-oriented and Mask-GCT are normal research terms".to_string());

        write_checkpoint(&run_dir, &run)
            .await
            .expect("benign sk- substrings should not be treated as secrets");
    }

    #[tokio::test]
    async fn checkpoint_rejects_probable_sk_secret_tokens() {
        let run_dir = temp_dir("checkpoint-secret-token");
        let mut run = ResearchRunRecord::new(
            "run-1".to_string(),
            "rust agent".to_string(),
            RunPolicy::default(),
        );
        run.state = ResearchRunState::PlanReady;
        run.warnings
            .push(format!("temporary key {}", fake_secret()));

        let err = write_checkpoint(&run_dir, &run)
            .await
            .expect_err("probable sk key should be rejected");

        assert!(err.to_string().contains("forbidden secret marker `sk-`"));
    }

    fn fake_secret() -> String {
        format!("sk-{}", "a".repeat(32))
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "litscout-rs-{name}-{}-{unique}",
            std::process::id()
        ))
    }
}
