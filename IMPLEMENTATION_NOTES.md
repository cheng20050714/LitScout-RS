# LitScout-RS 实现说明

## 当前主线

LitScout-RS 当前主线是 DeepSeek + GitHub/arXiv 网络抓取驱动的中文研究侦察 Agent。项目不维护离线产品模式，也不维护无 LLM 主流程；fixture/mock 只用于测试和 mini bench。

## 兼容工作流

仍然保留旧的线性 workflow：

1. CLI 输入中文主题。
2. 可选调用 DeepSeek 生成 SearchPlan。
3. 并发抓取 GitHub 与 arXiv。
4. 去重、排序、分类和 CitationLedger 构建。
5. DeepSeek 基于结构化来源生成中文分析。
6. 写入 Markdown 报告和 session JSON。

旧端点 `/api/plan`、`/api/run/stream`、`/api/report/chat`、`/api/report/translate` 保持兼容。

## Stage 3 Stateful Agent Workflow

第三阶段新增 `/api/runs/*` stateful workflow，核心模块：

- `workflow_state.rs`：`Created -> PlanReady -> Fetching -> EvidenceReady -> SynthesisReady -> Completed` 状态机。
- `run_policy.rs`：预算、章节上限、LLM 调用上限和可跳过节点。
- `trace.rs`：状态转移、LLM 节点 hash、工具调用、coverage gap、checkpoint 的 jsonl trace。
- `checkpoint.rs`：不含 API key 的 checkpoint snapshot。
- `agent/orchestrator.rs`：状态化编排器。
- `agent/scoper.rs`：生成 ResearchBrief。
- `agent/planner.rs`：生成扁平 ChapterPlan 和 QueryPortfolio。
- `agent/plan_critic.rs`：检查计划结构和查询覆盖。
- `agent/github_scout.rs` / `agent/arxiv_scout.rs`：受控双源抓取。
- `agent/academic_scout.rs`：在 `academic_extra_enabled` 开启时执行 Semantic Scholar / DBLP Stage A 扩源。
- `agent/evidence.rs`：构建 EvidenceMemory 和 query lineage。
- `agent/coverage_critic.rs`：输出 QueryGap / SourceGap。
- `agent/writer.rs`：根据 EvidenceMemory 生成中文报告草稿。
- `agent/citation_auditor.rs`：粗粒度 URL 白名单、引用覆盖和来源多样性审计。
- `agent/followup_router.rs`：基于当前 evidence/report 回答追问或提示增量研究。
- `agent/middleware.rs`：CitationGuard、JsonSchemaGuard、TokenBudgetTracker。

## 前端实现

Web 工作台已切换为 Stage 3 Agent 控制台：

- 配置阶段：DeepSeek API Key、GitHub Token、模型和 base URL。
- Agent 阶段：创建 stateful run。
- Plan Tree：编辑 ResearchBrief 后的章节计划和双源 query。
- Run Timeline：展示状态机、事件流和 checkpoint。
- Evidence Memory：按章节展示证据、原始链接、query lineage 和抓取错误。
- Coverage Matrix：展示 QueryGap、SourceGap 和建议查询。
- Citation Audit：展示 URL 白名单、段落引用覆盖率和警告。
- Report：Markdown 渲染和可选中文翻译。
- FollowupRouter：基于当前 EvidenceMemory 和报告追问。

## 安全边界

- LLM 不允许自行联网。
- LLM 不允许引用 CitationLedger / EvidenceMemory 之外的 URL。
- Stage A 扩源默认关闭；只有 CLI `--academic-extra` 或 Web policy 显式开启时才调用 Semantic Scholar / DBLP。
- Semantic Scholar / DBLP 只作为结构化学术来源进入统一 EvidenceMemory，不引入开放 Web 搜索。
- DBLP 记录按“书目元数据”弱证据处理，不作为强摘要来源。
- 不保存 `DEEPSEEK_API_KEY`、`GITHUB_TOKEN` 或完整 chain-of-thought。
- rollback 不修改旧 run；从 checkpoint 创建新 run 分支。
- Stage 3 不做自由 ReAct、不做任意网页搜索、不做 PDF 全文解析、不接外部 agent runtime。

## 验证

常规检查：

```bash
cargo fmt
cargo check
cargo test
cd web
npm run build
```

Stage 3 mini bench：

```bash
node scripts/stage3_eval.mjs
```

结果写入 `eval/results/stage3-mini-bench.json`。该 bench 只验证控制环结构和 fixture 门槛，不替代真实网络调研。
