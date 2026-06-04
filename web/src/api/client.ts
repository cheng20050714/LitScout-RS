import type {
  FrontendConfig,
  HealthResponse,
  ChatStreamEvent,
  ChapterNode,
  CheckpointListResponse,
  CitationAuditResponse,
  CoverageResponse,
  EvidenceResponse,
  PlanRequest,
  PlanResponse,
  QueryPortfolio,
  ReportTranslateResponse,
  ReportChatResponse,
  RunPolicy,
  RunEvent,
  RunResponse,
  StatefulFollowupResponse,
  StatefulRunResponse,
  StatefulRunStreamEvent
} from "./types";

async function readJson<T>(response: Response): Promise<T> {
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const body = (await response.json()) as { error?: string };
      if (body.error) {
        message = body.error;
      }
    } catch {
      // Keep the HTTP status fallback.
    }
    throw new Error(message);
  }
  return response.json() as Promise<T>;
}

export async function getHealth(): Promise<HealthResponse> {
  const response = await fetch("/api/health");
  return readJson<HealthResponse>(response);
}

export async function createPlan(request: PlanRequest): Promise<PlanResponse> {
  const response = await fetch("/api/plan", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(request)
  });
  return readJson<PlanResponse>(response);
}

export async function revisePlan(
  currentPlan: PlanResponse,
  userFeedback: string,
  config: FrontendConfig
): Promise<PlanResponse> {
  const response = await fetch("/api/plan/revise", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      plan_id: currentPlan.plan_id,
      current_plan: currentPlan,
      user_feedback: userFeedback,
      config
    })
  });
  return readJson<PlanResponse>(response);
}

export async function runResearch(
  currentPlan: PlanResponse,
  config: FrontendConfig
): Promise<RunResponse> {
  const response = await fetch("/api/run", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      plan_id: currentPlan.plan_id,
      current_plan: currentPlan,
      language: "zh-CN",
      config
    })
  });
  return readJson<RunResponse>(response);
}

export async function runResearchStream(
  currentPlan: PlanResponse,
  config: FrontendConfig,
  onEvent: (event: RunEvent) => void
): Promise<RunResponse> {
  const response = await fetch("/api/run/stream", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      plan_id: currentPlan.plan_id,
      current_plan: currentPlan,
      language: "zh-CN",
      config
    })
  });
  if (!response.ok || !response.body) {
    return readJson<RunResponse>(response);
  }
  let finalResponse: RunResponse | null = null;
  await readSse(response, (event) => {
    const runEvent = event as RunEvent;
    onEvent(runEvent);
    if (runEvent.event === "report_ready") {
      finalResponse = runEvent.data as RunResponse;
    }
    if (runEvent.event === "run_failed") {
      const data = runEvent.data as { error?: string };
      throw new Error(data.error ?? "调研执行失败。");
    }
  });
  if (!finalResponse) {
    throw new Error("调研流结束但没有收到报告。");
  }
  return finalResponse;
}

export async function createStatefulRun(
  topic: string,
  policy: RunPolicy,
  config: FrontendConfig
): Promise<StatefulRunResponse> {
  const response = await fetch("/api/runs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ topic, policy, config })
  });
  return readJson<StatefulRunResponse>(response);
}

export async function reviseStatefulPlan(
  runId: string,
  chapters: ChapterNode[],
  queryPortfolio: QueryPortfolio[],
  userFeedback: string
): Promise<StatefulRunResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/revise-plan`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      chapters,
      query_portfolio: queryPortfolio,
      user_feedback: userFeedback
    })
  });
  return readJson<StatefulRunResponse>(response);
}

export async function continueStatefulRunStream(
  runId: string,
  onEvent: (event: StatefulRunStreamEvent) => void
): Promise<StatefulRunResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/approve-plan`, {
    method: "POST"
  });
  if (!response.ok || !response.body) {
    return readJson<StatefulRunResponse>(response);
  }
  let finalResponse: StatefulRunResponse | null = null;
  await readSse(response, (event) => {
    const runEvent = event as StatefulRunStreamEvent;
    onEvent(runEvent);
    if (runEvent.event === "run_ready") {
      finalResponse = runEvent.data as StatefulRunResponse;
    }
    if (runEvent.event === "run_failed") {
      const data = runEvent.data as { error?: string };
      throw new Error(data.error ?? "Stateful run 执行失败。");
    }
  });
  if (!finalResponse) {
    throw new Error("Stateful run 流结束但没有收到最终状态。");
  }
  return finalResponse;
}

export async function getStatefulEvidence(runId: string): Promise<EvidenceResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/evidence`);
  return readJson<EvidenceResponse>(response);
}

export async function getStatefulCoverage(runId: string): Promise<CoverageResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/coverage`);
  return readJson<CoverageResponse>(response);
}

export async function getStatefulCitationAudit(runId: string): Promise<CitationAuditResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/citation-audit`);
  return readJson<CitationAuditResponse>(response);
}

export async function getStatefulCheckpoints(runId: string): Promise<CheckpointListResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/checkpoints`);
  return readJson<CheckpointListResponse>(response);
}

export async function branchStatefulRun(
  runId: string,
  checkpointId: string
): Promise<StatefulRunResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/branch-from-checkpoint`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ checkpoint_id: checkpointId })
  });
  return readJson<StatefulRunResponse>(response);
}

export async function askStatefulRun(
  runId: string,
  question: string
): Promise<StatefulFollowupResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/follow-up`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ question })
  });
  return readJson<StatefulFollowupResponse>(response);
}

export async function askReport(
  question: string,
  reportMarkdown: string,
  config: FrontendConfig
): Promise<ReportChatResponse> {
  const response = await fetch("/api/report/chat", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      question,
      report_markdown: reportMarkdown,
      config
    })
  });
  return readJson<ReportChatResponse>(response);
}

export async function askReportStream(
  question: string,
  reportMarkdown: string,
  config: FrontendConfig,
  onEvent: (event: ChatStreamEvent) => void
): Promise<void> {
  const response = await fetch("/api/report/chat/stream", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      question,
      report_markdown: reportMarkdown,
      config
    })
  });
  if (!response.ok || !response.body) {
    await readJson<unknown>(response);
    return;
  }
  await readSse(response, (event) => {
    const chatEvent = event as ChatStreamEvent;
    onEvent(chatEvent);
    if (chatEvent.event === "failed") {
      throw new Error(chatEvent.data.error ?? "报告追问失败。");
    }
  });
}

export async function translateReport(
  reportMarkdown: string,
  config: FrontendConfig
): Promise<ReportTranslateResponse> {
  const response = await fetch("/api/report/translate", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      report_markdown: reportMarkdown,
      config
    })
  });
  return readJson<ReportTranslateResponse>(response);
}

async function readSse(
  response: Response,
  onEvent: (event: unknown) => void
): Promise<void> {
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error("浏览器无法读取流式响应。");
  }
  const decoder = new TextDecoder();
  let buffer = "";
  while (true) {
    const { value, done } = await reader.read();
    if (done) {
      break;
    }
    buffer += decoder.decode(value, { stream: true });
    const frames = buffer.split("\n\n");
    buffer = frames.pop() ?? "";
    for (const frame of frames) {
      const dataLines = frame
        .split("\n")
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).trimStart());
      if (dataLines.length === 0) {
        continue;
      }
      onEvent(JSON.parse(dataLines.join("\n")));
    }
  }
}
