# LitScout-RS

LitScout-RS 是一个以 Rust 为核心的研究侦察 Agent。当前主线不再维护离线 mock 或无 LLM 版本，项目目标是通过 DeepSeek 生成搜索计划，并真实调用 GitHub API 与 arXiv API 抓取主题资料，最后生成带引用链接的中文 Markdown 调研报告。

## 核心工作流

```text
中文调研主题
  -> DeepSeek 生成可审查 SearchPlan
  -> 用户确认或修改 GitHub/arXiv 查询方向
  -> Rust 后端并发抓取 GitHub 与 arXiv
  -> 去重、排序、分类、构建 CitationLedger
  -> DeepSeek 基于已抓取结构化数据生成中文分析
  -> 质量门检查引用、URL 和来源覆盖
  -> 生成中文 Markdown 报告
  -> 可选：报告生成后再次调用 DeepSeek 翻译为中文
  -> 可选：基于报告内容进行流式追问
```

LLM 只能分析 LitScout-RS 已抓取到的 GitHub/arXiv 数据，不允许自行联网、编造来源或修改原始引用。

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
2. 调研阶段：输入中文 prompt，生成 SearchPlan，编辑搜索方向，启动 GitHub/arXiv 调研，查看进度和报告。

当前 Web 能力：

- 中文 SearchPlan 生成与修订。
- GitHub/arXiv 抓取进度通过 SSE 事件展示。
- 中文 Markdown 报告预览。
- CitationLedger 引用账本展示。
- 报告生成后可选调用 DeepSeek 翻译为中文，翻译结果会校验原始 URL 是否保留。
- 报告追问支持流式展示，并用 Markdown 渲染回答。

## API 摘要

- `GET /api/health`：检查后端状态。
- `POST /api/plan`：生成中文 SearchPlan。
- `POST /api/plan/revise`：根据用户反馈修改 SearchPlan。
- `POST /api/run`：执行调研并一次性返回报告。
- `POST /api/run/stream`：执行调研并通过 SSE 返回分阶段事件。
- `POST /api/report/translate`：将报告翻译为中文并校验引用 URL。
- `POST /api/report/chat`：基于报告进行一次性问答。
- `POST /api/report/chat/stream`：基于报告进行流式问答。

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

## 当前限制

- 数据源仍限定为 GitHub 和 arXiv。
- 不做任意网页爬虫、浏览器 Agent、PDF 全文解析或自由 ReAct。
- 报告追问接口已经是 SSE 流式形态；当前实现是后端获得完整回答后按 Markdown 段落推送，后续可替换为 DeepSeek 原生 token streaming。
