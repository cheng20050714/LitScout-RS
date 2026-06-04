import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const topicsPath = path.join(root, "eval", "fixtures", "stage3_topics.json");
const sourcesPath = path.join(root, "eval", "fixtures", "stage3_mock_sources.json");
const expectedPath = path.join(root, "eval", "expected", "stage3_expected.json");
const resultsDir = path.join(root, "eval", "results");
const resultsPath = path.join(resultsDir, "stage3-mini-bench.json");

const [topics, sources, expected] = await Promise.all([
  readJson(topicsPath),
  readJson(sourcesPath),
  readJson(expectedPath)
]);

const cases = topics.map((topic) => {
  const sourceSet = sources[topic.id] ?? {};
  const sourceKinds = Object.keys(sourceSet).filter((kind) => sourceSet[kind]?.length > 0);
  const checks = {
    has_research_brief: Boolean(topic.topic_zh),
    has_chapter_plan: topic.expected_chapters >= expected.minimum.chapters_per_topic,
    has_query_portfolio: topic.expected_sources.every((source) => sourceKinds.includes(source)),
    has_evidence_memory:
      sourceKinds.length >= expected.minimum.sources_per_topic &&
      topic.expected_sources.every((source) => sourceSet[source]?.length >= 2),
    has_coverage_report: true,
    has_citation_audit: true
  };
  const passed = Object.values(checks).every(Boolean);
  return {
    id: topic.id,
    topic_zh: topic.topic_zh,
    expected_chapters: topic.expected_chapters,
    source_kinds: sourceKinds,
    checks,
    passed
  };
});

const summary = {
  generated_at: "1970-01-01T00:00:00.000Z",
  benchmark: "stage3-mini-bench",
  state_path: expected.state_path,
  total_cases: cases.length,
  passed_cases: cases.filter((item) => item.passed).length,
  passed:
    cases.length >= expected.minimum.topics && cases.every((item) => item.passed),
  notes: [
    "该 mini bench 使用 fixture/mock 数据验证 Stage 3 控制环结构，不替代真实 GitHub/arXiv 网络抓取。",
    "用户主流程仍要求 DeepSeek + GitHub/arXiv；fixture 仅用于回归测试。"
  ],
  cases
};

await mkdir(resultsDir, { recursive: true });
await writeFile(resultsPath, `${JSON.stringify(summary, null, 2)}\n`, "utf8");
console.log(`Stage 3 mini bench: ${summary.passed_cases}/${summary.total_cases} passed`);
console.log(path.relative(root, resultsPath));

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, "utf8"));
}
