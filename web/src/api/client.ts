import type {
  FrontendConfig,
  HealthResponse,
  ChatStreamEvent,
  PlanRequest,
  PlanResponse,
  ReportTranslateResponse,
  ReportChatResponse,
  RunEvent,
  RunResponse
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
