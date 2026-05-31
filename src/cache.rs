use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::config::AppConfig;
use crate::error::Result;
use crate::model::SearchQuery;

const CACHE_EXTENSION: &str = "json";

pub fn cache_key(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

pub fn cache_key_for(query: &SearchQuery, source: &str, limit: usize) -> String {
    let limit = limit.to_string();
    cache_key(&[query.topic.as_str(), source, limit.as_str()])
}

pub fn cache_path(cache_dir: PathBuf, key: &str, extension: &str) -> PathBuf {
    cache_dir.join(format!("{key}.{extension}"))
}

pub fn cache_file_path(
    cache_dir: &Path,
    query: &SearchQuery,
    source: &str,
    limit: usize,
) -> PathBuf {
    let key = cache_key_for(query, source, limit);
    cache_path(cache_dir.to_path_buf(), &key, CACHE_EXTENSION)
}

pub async fn load_source_cache<T>(
    config: &AppConfig,
    query: &SearchQuery,
    source: &str,
    limit: usize,
) -> Result<Option<Vec<T>>>
where
    T: DeserializeOwned,
{
    if !config.use_cache {
        return Ok(None);
    }

    let path = cache_file_path(&config.cache_dir, query, source, limit);
    let body = match tokio::fs::read_to_string(&path).await {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            warn!("Failed to read cache {}: {}", path.display(), err);
            return Ok(None);
        }
    };

    let entry = match serde_json::from_str::<SourceCache<T>>(&body) {
        Ok(entry) => entry,
        Err(err) => {
            warn!("Ignoring corrupted cache {}: {}", path.display(), err);
            return Ok(None);
        }
    };

    if entry.source != source || entry.limit != limit || entry.query.topic != query.topic {
        warn!(
            "Ignoring cache {} because metadata does not match",
            path.display()
        );
        return Ok(None);
    }

    if is_expired(entry.written_at, config.cache_ttl_hours) {
        return Ok(None);
    }

    Ok(Some(entry.items))
}

pub async fn save_source_cache<T>(
    config: &AppConfig,
    query: &SearchQuery,
    source: &str,
    limit: usize,
    items: &[T],
) -> Result<()>
where
    T: Serialize + Clone,
{
    if !config.use_cache {
        return Ok(());
    }

    tokio::fs::create_dir_all(&config.cache_dir).await?;
    let path = cache_file_path(&config.cache_dir, query, source, limit);
    let entry = SourceCache {
        written_at: Utc::now(),
        query: query.clone(),
        source: source.to_string(),
        limit,
        items: items.to_vec(),
    };
    let body = serde_json::to_string_pretty(&entry)?;
    tokio::fs::write(path, body).await?;
    Ok(())
}

fn is_expired(written_at: DateTime<Utc>, ttl_hours: u64) -> bool {
    let elapsed = Utc::now().signed_duration_since(written_at);
    let ttl = chrono::Duration::hours(ttl_hours.min(i64::MAX as u64) as i64);
    elapsed > ttl
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceCache<T> {
    written_at: DateTime<Utc>,
    query: SearchQuery,
    source: String,
    limit: usize,
    items: Vec<T>,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{Duration, Utc};
    use serde_json::json;

    use super::{cache_file_path, cache_key, load_source_cache, save_source_cache, SourceCache};
    use crate::config::AppConfig;
    use crate::model::{GitHubRepo, SearchQuery};

    #[test]
    fn cache_key_is_stable() {
        assert_eq!(
            cache_key(&["topic", "github"]),
            cache_key(&["topic", "github"])
        );
        assert_ne!(
            cache_key(&["topic", "github"]),
            cache_key(&["topic", "arxiv"])
        );
    }

    #[tokio::test]
    async fn cache_hit_returns_items() {
        let (config, query) = test_config("hit", 24);
        let repo = sample_repo();

        save_source_cache(&config, &query, "github", 10, std::slice::from_ref(&repo))
            .await
            .expect("cache write should succeed");

        let cached: Option<Vec<GitHubRepo>> = load_source_cache(&config, &query, "github", 10)
            .await
            .expect("cache read should succeed");

        assert_eq!(cached, Some(vec![repo]));
    }

    #[tokio::test]
    async fn cache_miss_for_absent_file() {
        let (config, query) = test_config("miss", 24);

        let cached: Option<Vec<GitHubRepo>> = load_source_cache(&config, &query, "github", 10)
            .await
            .expect("cache miss should not fail");

        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn cache_expired_returns_none() {
        let (config, query) = test_config("expired", 1);
        fs::create_dir_all(&config.cache_dir).expect("temp cache dir should be creatable");
        let path = cache_file_path(&config.cache_dir, &query, "github", 10);
        let entry = SourceCache {
            written_at: Utc::now() - Duration::hours(2),
            query: query.clone(),
            source: "github".to_string(),
            limit: 10,
            items: vec![sample_repo()],
        };
        fs::write(path, serde_json::to_string(&entry).unwrap()).unwrap();

        let cached: Option<Vec<GitHubRepo>> = load_source_cache(&config, &query, "github", 10)
            .await
            .expect("expired cache should not fail");

        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn corrupted_cache_falls_back_to_miss() {
        let (config, query) = test_config("corrupt", 24);
        fs::create_dir_all(&config.cache_dir).expect("temp cache dir should be creatable");
        let path = cache_file_path(&config.cache_dir, &query, "github", 10);
        fs::write(path, "{not valid json").unwrap();

        let cached: Option<Vec<GitHubRepo>> = load_source_cache(&config, &query, "github", 10)
            .await
            .expect("corrupted cache should not fail");

        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn no_cache_disables_read_and_write() {
        let (mut config, query) = test_config("disabled", 24);
        config.use_cache = false;

        save_source_cache(&config, &query, "github", 10, &[sample_repo()])
            .await
            .expect("disabled cache write should be a no-op");
        let cached: Option<Vec<GitHubRepo>> = load_source_cache(&config, &query, "github", 10)
            .await
            .expect("disabled cache read should be a no-op");

        assert!(cached.is_none());
        assert!(!cache_file_path(&config.cache_dir, &query, "github", 10).exists());
    }

    #[test]
    fn cache_file_path_uses_query_source_and_limit() {
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        let base = PathBuf::from(".litscout-cache");

        let github_10 = cache_file_path(&base, &query, "github", 10);
        let github_20 = cache_file_path(&base, &query, "github", 20);
        let arxiv_10 = cache_file_path(&base, &query, "arxiv", 10);

        assert_ne!(github_10, github_20);
        assert_ne!(github_10, arxiv_10);
        assert_eq!(
            github_10.extension().and_then(|ext| ext.to_str()),
            Some("json")
        );
    }

    fn test_config(name: &str, ttl_hours: u64) -> (AppConfig, SearchQuery) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let cache_dir = std::env::temp_dir().join(format!(
            "litscout-rs-cache-{name}-{}-{unique}",
            std::process::id()
        ));
        let config = AppConfig {
            github_token: None,
            output: cache_dir.join("report.md"),
            cache_dir,
            session_dir: PathBuf::from("sessions"),
            tags_file: None,
            use_cache: true,
            cache_ttl_hours: ttl_hours,
            timeout_secs: 30,
            enrich: false,
        };
        let query = SearchQuery {
            topic: "rust agent".to_string(),
            github_limit: 10,
            arxiv_limit: 10,
        };
        (config, query)
    }

    fn sample_repo() -> GitHubRepo {
        serde_json::from_value(json!({
            "owner": "acme",
            "name": "rust-agent",
            "full_name": "acme/rust-agent",
            "html_url": "https://github.com/acme/rust-agent",
            "description": "Rust agent framework",
            "stars": 42,
            "forks": 3,
            "language": "Rust",
            "updated_at": "2026-05-30T12:00:00Z",
            "topics": ["rust", "agent"],
            "readme_excerpt": null
        }))
        .unwrap()
    }
}
