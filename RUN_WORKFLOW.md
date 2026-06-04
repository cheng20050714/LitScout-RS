# LitScout-RS 修改后运行流程

这份文档用于每次修改代码后快速检查、启动和排错。

## 0. 最短流程

只改 Rust：

```bash
cargo fmt
cargo check
cargo test
```

改了前端：

```bash
cd web
npm run build
cd ..
```

改了 Stage 3 agent 控制环：

```bash
node scripts/stage3_eval.mjs
```

启动 Web 工作台：

```bash
cargo run -- --serve --llm --port 3000
```

如果不想通过环境变量传 DeepSeek Key，也可以启动：

```bash
cargo run -- --serve --port 3000
```

然后在前端第一阶段填写 DeepSeek API Key。

## 1. 进入项目目录

```bash
cd /Users/cheng/NKU/大二下/Rust/LitScout-RS/litscout-rs
```

## 2. 配置环境变量

DeepSeek 是当前主流程必需配置。推荐用环境变量配置，并在启动 Web 服务时带上 `--llm`：

```bash
export DEEPSEEK_API_KEY="你的 DeepSeek API Key"
export DEEPSEEK_BASE_URL="https://api.deepseek.com"
export DEEPSEEK_MODEL="deepseek-v4-pro"
export DEEPSEEK_SIDE_MODEL="deepseek-v4-flash"
```

如果你计划在前端第一阶段手动填写 DeepSeek API Key，可以不提前 export。

GitHub Token 可选，但如果配置了错误 token，会导致 GitHub `401 Bad credentials`。不确定 token 是否正确时，先清空：

```bash
unset GITHUB_TOKEN
```

确认 token 有效后再配置：

```bash
export GITHUB_TOKEN="你的 GitHub Token"
```

## 3. Rust 后端验证

每次改 Rust 代码后运行：

```bash
cargo fmt
cargo check
cargo test
```

至少需要 `cargo check` 通过；正式运行前建议确认 `cargo test` 全部通过。

## 4. 前端验证

如果修改了 `web/` 下的前端代码，运行：

```bash
cd web
npm run build
cd ..
```

注意：`npm run build` 必须在 `web/` 目录执行。

## 5. 启动 Web 工作台

如果之前服务还在运行，先停止旧服务。常见端口占用时可以换端口。

使用环境变量中的 DeepSeek 配置：

```bash
cargo run -- --serve --llm --port 3000
```

使用前端配置页填写 DeepSeek Key：

```bash
cargo run -- --serve --port 3000
```

打开：

```text
http://127.0.0.1:3000
```

如果 3000 被占用：

```bash
cargo run -- --serve --llm --port 3001
```

然后打开：

```text
http://127.0.0.1:3001
```

## 6. Web 使用流程

1. 阶段 1：填写 DeepSeek API Key。
2. 可选填写 GitHub Token；如果不确定是否有效，先留空。
3. 保存配置进入 Agent 阶段。
4. 输入中文研究问题，设置 GitHub/arXiv 预算和章节上限。
5. 点击“创建 Agent Run”，生成 ResearchBrief、ChapterPlan 和 QueryPortfolio。
6. 在“计划”页检查并修改章节、GitHub queries、arXiv queries。
7. 点击“保存计划修订”会写入新的 PlanReady checkpoint。
8. 点击“批准并开始调研”，后端会进入 `Fetching -> EvidenceReady -> SynthesisReady -> Completed`。
9. 右侧 Run Timeline 会显示状态机、事件流和 checkpoint。
10. “证据”页查看 EvidenceMemory、query lineage 和抓取错误。
11. “覆盖”页查看 QueryGap / SourceGap。
12. “审计”页查看 CitationAudit 结果。
13. “报告”页查看 Markdown 渲染结果。
14. 可选点击“用 LLM 翻译为中文”。
15. 可在“追问”页基于当前 EvidenceMemory 和报告继续提问。
16. 如需回退，在右侧 PlanReady checkpoint 点击“创建分支”；旧 run 不会被修改。

## 7. CLI 运行

CLI 模式需要 `--llm`：

```bash
cargo run -- "搜索 TTS 领域最新开源项目和论文" --llm
```

指定输出：

```bash
cargo run -- "rust agent framework" --llm --output reports/rust-agent.md
```

限制抓取数量：

```bash
cargo run -- "llm tool calling" --llm --github-limit 8 --arxiv-limit 8
```

## 8. 常见问题

### GitHub 401 Bad credentials

原因：GitHub Token 错误、过期或被撤销。

处理：

```bash
unset GITHUB_TOKEN
```

前端配置页也清空 GitHub Token，再重新运行。

### arXiv 429 Rate exceeded

原因：arXiv 服务端限流。

当前代码已经加入退避重试和 aspect 间隔，但连续多次运行仍可能触发限流。

处理建议：

- 等几分钟后再试。
- 降低 `--arxiv-limit`。
- 避免频繁重复运行同一个主题。
- 保持缓存开启，不要频繁使用 `--no-cache`。

### DeepSeek error decoding response body

原因通常是 DeepSeek 响应传输或压缩解码异常。

当前代码已经请求非压缩响应并增加重试。如果仍出现：

- 稍后重试。
- 增大超时时间。
- 检查网络代理是否影响 HTTPS 响应。

示例：

```bash
cargo run -- "TTS 调研" --llm --llm-timeout 60
```

## 9. 修改后提交前检查

提交前建议运行：

```bash
cargo fmt
cargo check
cargo test
node scripts/stage3_eval.mjs
cd web
npm run build
cd ..
git status --short
```

确认没有把 `.env`、真实 API Key、`target/`、`web/dist/`、`reports/`、`sessions/` 提交进去。
