# LitScout-RS

LitScout-RS 是一个以 Rust 为核心的研究侦察 Agent。当前主线不再维护离线 mock 或无 LLM 版本，项目目标是通过 DeepSeek 生成搜索计划，并真实调用 GitHub API 与 arXiv API 抓取主题资料，最后生成带引用链接的中文 Markdown 调研报告。

## 核心工作流

```text
中文调研主题
  -> Scoper 生成 ResearchBrief
  -> Planner 生成扁平 ChapterPlan + QueryPortfolio
  -> PlanCritic 给出计划警告和建议
  -> 用户确认或修改计划
  -> GitHubScout / ArxivScout 执行受控抓取
  -> EvidenceBuilder 构建 EvidenceMemory 和 query lineage
  -> CoverageCritic 标记 QueryGap / SourceGap
  -> Writer 生成带引用标注的中文 Markdown 报告
  -> CitationAuditor 做 URL 白名单、引用覆盖和来源多样性审计
  -> 可选：报告生成后再次调用 DeepSeek 翻译为中文
  -> 可选：基于当前 EvidenceMemory 和报告内容追问
```

LLM 只能分析 LitScout-RS 已抓取到的 GitHub/arXiv 数据，不允许自行联网、编造来源或修改原始引用。

## Stage 3 Agent 控制环

第三阶段新增了并行于旧线性 workflow 的 stateful run 路径，API 前缀为 `/api/runs/*`。旧 CLI、`/api/plan` 和 `/api/run/stream` 仍保留兼容。

状态机：

```text
Created -> PlanReady -> Fetching -> EvidenceReady -> SynthesisReady -> Completed
                                                             \-> Failed
```

新增能力：

- 每次运行都有 `run_id`，摘要写入 `sessions/<run_id>.json`。
- 每次状态转移、工具调用、LLM 节点输入输出 hash、checkpoint 都写入 `sessions/<run_id>/trace.jsonl`。
- `PlanReady`、`EvidenceReady`、`SynthesisReady`、`Completed` 会写 checkpoint。
- rollback 不修改旧 run，而是从 `PlanReady` checkpoint 创建新 run 分支。
- 前端新增 Run Timeline、Plan Tree、Evidence Memory、Coverage Matrix、Citation Audit 和 Checkpoint/Branch 视图。
- `RunPolicy` 控制章节数、GitHub/arXiv 预算、LLM 调用上限、是否跳过 PlanCritic/CoverageCritic、是否要求 citation audit。

## 环境变量

```bash
export DEEPSEEK_API_KEY=...
export DEEPSEEK_BASE_URL=https://api.deepseek.com
export DEEPSEEK_MODEL=deepseek-v4-pro
export DEEPSEEK_SIDE_MODEL=deepseek-v4-flash
export DEEPSEEK_MAX_TOKENS=4096
export DEEPSEEK_TIMEOUT_SECS=30

# 可选，但建议配置以提升 GitHub API rate limit
export GITHUB_TOKEN=...
```

不要把真实 API Key 写入代码、README、日志或提交记录。

## CLI 运行

CLI 模式需要启用 `--llm`：

```bash
cargo run -- "rust agent framework" --llm
cargo run -- "llm tool calling" --llm --github-limit 10 --arxiv-limit 10
cargo run -- "code agent benchmark" --llm --output reports/agent.md
cargo run -- "retrieval augmented generation" --llm --no-cache
```

也可以直接传入 DeepSeek 参数：

```bash
cargo run -- "rust agent framework" --llm \
  --deepseek-api-key "$DEEPSEEK_API_KEY" \
  --deepseek-base-url https://api.deepseek.com \
  --deepseek-model deepseek-v4-pro \
  --deepseek-side-model deepseek-v4-flash \
  --llm-timeout 45 \
  --llm-max-tokens 4096
```

报告默认写入 `reports/<topic>-<timestamp>.md`，每次运行还会在 `sessions/` 写入不包含密钥的 session JSON。

## Web 工作台

先构建前端：

```bash
cd web
npm install
npm run build
cd ..
```

启动 Rust 服务：

```bash
cargo run -- --serve --port 3000
```

打开：

```text
http://127.0.0.1:3000
```

Web 工作台包含两个阶段：

1. 配置阶段：填写 DeepSeek API Key、GitHub Token、DeepSeek base URL 和模型名。
2. Agent 阶段：输入中文 prompt，创建 stateful run，审查 Plan Tree，批准后启动 GitHub/arXiv 调研，查看状态机进度、证据、覆盖度、引用审计和报告。

当前 Web 能力：

- 中文 ResearchBrief、ChapterPlan 与 QueryPortfolio 生成。
- Plan Tree 可编辑并保存为新的 PlanReady checkpoint。
- GitHub/arXiv 抓取进度通过 SSE 事件展示。
- EvidenceMemory 按章节展示证据、来源链接和 query lineage。
- Coverage Matrix 区分 QueryGap 与 SourceGap。
- Citation Audit 展示 URL 白名单、段落引用覆盖率、来源多样性和警告。
- Checkpoint/Branch 支持从 PlanReady checkpoint 创建新 run。
- 中文 Markdown 报告预览。
- 报告生成后可选调用 DeepSeek 翻译为中文，翻译结果会校验原始 URL 是否保留。
- 报告追问使用真实 LLM 流式输出，并用 Markdown 渲染回答。
- 阅读库支持从 arXiv 证据加入论文、生成深度阅读笔记和单篇论文流式追问。

## API 摘要

- `GET /api/health`：检查后端状态。
- `POST /api/plan`：生成中文 SearchPlan。
- `POST /api/plan/revise`：根据用户反馈修改 SearchPlan。
- `POST /api/run`：执行调研并一次性返回报告。
- `POST /api/run/stream`：执行调研并通过 SSE 返回分阶段事件。
- `POST /api/report/translate`：将报告翻译为中文并校验引用 URL。
- `POST /api/report/chat`：基于报告进行一次性问答。
- `POST /api/report/chat/stream`：基于报告进行流式问答。

Stage 3 新增：

- `POST /api/runs`：创建 stateful run，返回 `run_id`、ResearchBrief、ChapterPlan 和 QueryPortfolio。
- `GET /api/runs/:run_id`：读取当前 run 摘要。
- `GET /api/runs/:run_id/events`：读取当前 run 状态事件。
- `POST /api/runs/:run_id/approve-plan`：批准 PlanReady 并通过 SSE 推进抓取、证据构建、写作和审计。
- `POST /api/runs/:run_id/revise-plan`：保存用户编辑后的 ChapterPlan / QueryPortfolio，并写入新 checkpoint。
- `GET /api/runs/:run_id/evidence`：读取 EvidenceMemory。
- `GET /api/runs/:run_id/coverage`：读取 CoverageReport。
- `GET /api/runs/:run_id/citation-audit`：读取 CitationAuditReport。
- `GET /api/runs/:run_id/checkpoints`：列出 checkpoint。
- `POST /api/runs/:run_id/branch-from-checkpoint`：从 PlanReady checkpoint 创建新 run。

Stage 4 新增：

- `GET /api/library`：列出阅读库论文。
- `POST /api/library/items`：从 arXiv EvidenceItem 加入阅读库。
- `GET /api/library/items/:paper_key`：读取单篇阅读库论文。
- `DELETE /api/library/items/:paper_key`：删除单篇阅读库论文。
- `POST /api/library/items/:paper_key/generate-note`：抓取论文可读文本并生成深度阅读笔记。
- `POST /api/library/items/:paper_key/chat/stream`：基于单篇论文进行真实流式问答。

旧的 `POST /api/runs/:run_id/follow-up` 已删除；调研页不再保留第二阶段“追问”链路。

## Rust 特性

- `tokio::join!` 并发请求 GitHub 与 arXiv。
- `reqwest` 调用 GitHub、arXiv 和 DeepSeek。
- `serde` 处理 API、缓存、session、LLM JSON。
- `roxmltree` 解析 arXiv Atom XML。
- `clap` 构建 CLI。
- `thiserror` 管理统一错误类型。
- `axum` 提供 Web API 与 SSE 事件流。
- 本地 JSON 缓存减少重复请求。
- 模块化组织 source、ranking、classification、report、quality、session、LLM。

## 开发检查

Rust：

```bash
cargo fmt
cargo check
cargo test
```

前端：

```bash
cd web
npm run build
```

Stage 3 mini bench：

```bash
node scripts/stage3_eval.mjs
```

结果写入 `eval/results/stage3-mini-bench.json`。该 bench 使用 fixture/mock 数据验证控制环结构，不替代真实 GitHub/arXiv 网络抓取。

## 当前限制

- 数据源仍限定为 GitHub 和 arXiv。
- 不做任意网页爬虫、浏览器 Agent、PDF 全文解析或自由 ReAct。
- Stage 3 只实现粗粒度 citation audit；claim-level audit 延后。
- CoverageCritic 只建议补抓，不自动进入开放式循环。
- rollback 第一版只支持从 `PlanReady` checkpoint 创建新 run。
- 报告追问接口已经是 SSE 流式形态；当前实现是后端获得完整回答后按 Markdown 段落推送，后续可替换为 DeepSeek 原生 token streaming。
