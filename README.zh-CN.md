# LitScout-RS

<p align="center">
  <a href="README.md">English</a> | 简体中文
</p>

<p align="center">
  <img src="icon.png" alt="LitScout-RS logo" width="132" />
</p>

<p align="center">
  <strong>用 Rust 构建的证据约束型研究侦察 Agent。</strong>
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img alt="Rust" src="https://img.shields.io/badge/Rust-2021-b7410e?logo=rust&logoColor=white"></a>
  <img alt="Status" src="https://img.shields.io/badge/status-active-brightgreen">
  <img alt="LLM" src="https://img.shields.io/badge/LLM-DeepSeek-4b6bfb">
  <img alt="Sources" src="https://img.shields.io/badge/sources-GitHub%20%7C%20arXiv%20%7C%20Academic%20Indexes-0f766e">
  <img alt="License" src="https://img.shields.io/badge/license-not%20set-lightgrey">
</p>

LitScout-RS 是一个基于 Rust 的研究侦察 Agent，可以把一个研究主题转化为可审查、带引用的调研报告，并继续进入论文阅读笔记与单篇追问流程。

项目面向课程调研、早期文献综述、开源技术选型和研究型开发。它不把自己设计成无限制浏览器 Agent，而是从受控来源收集证据，记录每条证据的查询来源，筛掉噪声较高的学术候选，审计引用边界，并允许用户从生成报告继续进入论文级阅读笔记和问答。

## 项目亮点

- **完整研究流程，而不只是搜索列表**：生成研究简介、章节计划、查询组合、证据池、覆盖度报告、引用审计和最终 Markdown 报告。
- **受控多源收集**：默认使用 GitHub 和 arXiv，可显式启用 Semantic Scholar、DBLP、OpenAlex、Crossref 组成的 Stage A 学术扩展源。
- **证据优先的报告生成**：写作模块只能引用登记在 `CitationLedger` 中的 URL，来源 lineage 和 query attempt 均可检查。
- **准确率优先的扩源策略**：学术候选需要经过 canonical merge、ranking、classification 和 `EvidenceQualityGate`，才能进入报告证据池。
- **有状态的 Agent 控制环**：状态迁移、trace 事件、checkpoint 和 PlanReady 分支让每次运行都可复盘，而不是只看到一个黑盒结果。
- **内置论文阅读库**：证据池中的 arXiv 论文可以加入阅读库，继续做全文或近全文抓取、结构化笔记和单篇论文追问。
- **Rust 后端和类型化边界**：API 响应、session、checkpoint、证据模型和 LLM 输出都用 Rust 类型与 serde 兼容 JSON 表达。

## 系统工作流

<p align="center">
  <img src="workflow.png" alt="LitScout-RS workflow" width="860" />
</p>

```text
User research topic
  -> Scoper creates ResearchBrief
  -> Planner creates ChapterPlan and QueryPortfolio
  -> PlanCritic reviews plan quality
  -> User approves or revises the plan
  -> GitHubScout / ArxivScout collect default evidence
  -> Optional AcademicScout collects Semantic Scholar / DBLP / OpenAlex / Crossref
  -> EvidenceBuilder performs canonical merge, ranking, classification, and EvidenceQualityGate
  -> EvidenceMemory and CitationLedger are built with query lineage
  -> CoverageCritic checks chapter-level evidence gaps
  -> Writer generates a cited Markdown report
  -> CitationAuditor checks URL whitelist, citation coverage, and source diversity
  -> Reading Library supports arXiv paper notes and single-paper Q&A
```

LLM 不会被允许自由浏览网页或编造来源。它只能接收 LitScout-RS 已收集并登记的证据，最终报告中的引用也会被 `CitationLedger` 约束和检查。

## 架构概览

```text
src/
  main.rs                 # 程序入口和 Web server 启动
  cli.rs                  # 命令行参数
  config.rs               # AppConfig 和环境变量处理
  model.rs                # SourceItem、EvidenceMemory、CitationLedger、run model
  sources/                # GitHub、arXiv、Semantic Scholar、DBLP、OpenAlex、Crossref
  agent/                  # scoper、planner、scout、evidence builder、writer、auditor
  reading/                # arXiv 阅读库、正文抓取、笔记、单篇论文问答
  server/                 # axum 路由和 SSE endpoint
  ranking.rs              # 排序信号
  dedup.rs                # canonical work merge
  classify.rs             # 证据分类
  checkpoint.rs           # 不持久化密钥的 checkpoint
  trace.rs                # jsonl run trace
web/src/
  components/             # React 工作台视图
  api/                    # 类型化前端 API client
```

## 环境要求

- Rust stable toolchain 和 Cargo
- Node.js 与 npm，用于 Web 工作台
- DeepSeek 兼容的 chat completion endpoint
- 可选 GitHub token，用于提高 GitHub API rate limit

## 配置

LitScout-RS 从 CLI 参数和环境变量读取运行配置。参考配置写在 `config.example.toml` 中。

```bash
export DEEPSEEK_API_KEY=...
export DEEPSEEK_BASE_URL=https://api.deepseek.com
export DEEPSEEK_MODEL=deepseek-v4-pro
export DEEPSEEK_SIDE_MODEL=deepseek-v4-flash
export DEEPSEEK_MAX_TOKENS=4096
export DEEPSEEK_TIMEOUT_SECS=30

# 可选，建议用于提高 GitHub API rate limit。
export GITHUB_TOKEN=...

# 可选学术扩展源。
export SEMANTIC_SCHOLAR_API_KEY=...
export OPENALEX_API_KEY=...
export CROSSREF_MAILTO=you@example.com
```

不要提交真实 API key。checkpoint 和 trace 的设计目标是不持久化密钥，但环境变量和本地 shell history 仍需要正常保护。

## 快速开始：Web 工作台

构建前端并启动 Rust server：

```bash
cd web
npm install
npm run build
cd ..

cargo run -- --serve --port 3000
```

打开：

```text
http://127.0.0.1:3000
```

推荐的 Web 使用流程：

1. 配置 DeepSeek 和可选 GitHub token。
2. 输入中文或英文主题创建调研任务。
3. 在正式收集前审查生成的计划。
4. 批准计划，观察运行进度、事件和 checkpoint。
5. 检查证据、query lineage、覆盖度和引用审计。
6. 阅读生成的 Markdown 报告。
7. 将有价值的 arXiv 论文加入阅读库，继续生成笔记和单篇论文问答。

学术扩展源需要显式启用。只有当你希望 Semantic Scholar、DBLP、OpenAlex 和 Crossref 参与证据池时，才在工作台 policy 中打开对应能力。

## CLI 使用

当前主流程需要 LLM mode：

```bash
cargo run -- "rust agent framework" --llm
cargo run -- "llm tool calling" --llm --github-limit 10 --arxiv-limit 10
cargo run -- "llm agent benchmark" --llm --academic-extra --academic-limit 10
```

需要时也可以通过命令行传入 DeepSeek 配置：

```bash
cargo run -- "rust agent framework" --llm \
  --deepseek-api-key "$DEEPSEEK_API_KEY" \
  --deepseek-base-url https://api.deepseek.com \
  --deepseek-model deepseek-v4-pro \
  --deepseek-side-model deepseek-v4-flash \
  --llm-timeout 45 \
  --llm-max-tokens 4096
```

报告会写入 `reports/<topic>-<timestamp>.md`。session summary、trace 和 checkpoint 会写入 `sessions/`，其中不会保存 API key。

## Web 工作台能力

- 证据收集前的计划审查和修订。
- 基于 SSE 的运行进度展示，覆盖 fetching、evidence building、synthesis 和 audit 阶段。
- 证据池展示 source link、query attempt、lineage 和 selection summary。
- 覆盖度矩阵展示章节级 `QueryGap` 与 `SourceGap` 诊断。
- 引用审计检查 URL 白名单、引用覆盖度和来源多样性。
- checkpoint 列表和 PlanReady 分支创建。
- Markdown 报告预览。
- 阅读库支持 arXiv 论文入库、正文抓取诊断、结构化笔记和单篇论文流式问答。

## API 概览

核心 endpoints：

- `GET /api/health`
- `POST /api/plan`
- `POST /api/plan/revise`
- `POST /api/run`
- `POST /api/run/stream`
- `POST /api/report/translate`

有状态运行 endpoints：

- `POST /api/runs`
- `GET /api/runs/:run_id`
- `GET /api/runs/:run_id/events`
- `POST /api/runs/:run_id/approve-plan`
- `POST /api/runs/:run_id/continue`
- `POST /api/runs/:run_id/revise-plan`
- `GET /api/runs/:run_id/evidence`
- `GET /api/runs/:run_id/coverage`
- `GET /api/runs/:run_id/citation-audit`
- `GET /api/runs/:run_id/checkpoints`
- `POST /api/runs/:run_id/branch-from-checkpoint`

阅读库 endpoints：

- `GET /api/library`
- `POST /api/library/items`
- `GET /api/library/items/:paper_key`
- `DELETE /api/library/items/:paper_key`
- `POST /api/library/items/:paper_key/generate-note`
- `POST /api/library/items/:paper_key/chat/stream`

报告级追问 endpoints 已移除。追问现在属于阅读库，语境限定在单篇论文内部。

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

Stage 3 fixture bench：

```bash
node scripts/stage3_eval.mjs
```

这个 mini bench 用 fixture/mock data 验证控制环结构，不能替代 GitHub、arXiv、学术扩展源和 LLM 的真实集成检查。

## 当前边界

- 默认来源是 GitHub 和 arXiv。
- Stage A 学术扩展源需要通过 `--academic-extra` 或 Web run policy 显式启用。
- 开放 Web 搜索、浏览器自动化、无限制 ReAct、微信公众号抓取和通用新闻抓取都不在当前范围内。
- Academic index 和 bibliography 候选必须通过 `EvidenceQualityGate`，才能进入 `EvidenceMemory` 和 `CitationLedger`。
- 阅读库当前聚焦 arXiv 论文。它可以通过 Jina Reader 和本地 PDF extraction 抓取正文，但不是通用 PDF/文档分析系统。
- `CoverageCritic` 负责报告缺口和建议，不会启动开放式自主爬取循环。
- 分支能力目前支持从 PlanReady checkpoint 创建新 run。
- 论文问答使用 SSE 接口。当前后端可能先缓冲模型输出，再发送 Markdown chunk；后续可以补 token-native streaming。

## 仓库维护建议

推荐的 GitHub project settings：

- 添加简短仓库描述，比如：`Rust research scouting agent with evidence-gated multi-source search, cited reports, and arXiv reading notes.`
- 添加 topics，例如 `rust`、`llm`、`research-agent`、`literature-review`、`arxiv`、`github-api`、`evidence`、`citation-audit`。
- 如果仓库准备公开发布或复用，请添加 license file。
- 不要提交 `.env`、API key、生成的 sessions、本地 cache 和私有 reports。
- 如果希望 README 在 GitHub 上展示截图或架构图，建议把图片放在可追踪的 `docs/assets/` 目录下。

## License / 许可证
