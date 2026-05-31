#!/usr/bin/env bash
set -euo pipefail

cargo run -- "rust agent framework" --github-limit 5 --arxiv-limit 5 --output reports/rust-agent-framework.md

if [[ -n "${DEEPSEEK_API_KEY:-}" ]]; then
  cargo run -- "llm tool calling" --llm --github-limit 5 --arxiv-limit 5 --output reports/llm-tool-calling.md
else
  echo "DEEPSEEK_API_KEY is not set; skipping LLM demo run."
fi
