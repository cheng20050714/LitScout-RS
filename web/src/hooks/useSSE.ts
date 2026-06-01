import { useCallback, useState } from "react";
import type { RunEvent } from "../api/types";

export function useSSE() {
  const [events, setEvents] = useState<RunEvent[]>([]);
  const [status, setStatus] = useState<"idle" | "running" | "completed" | "failed">("idle");

  const reset = useCallback(() => {
    setEvents([]);
    setStatus("idle");
  }, []);

  const appendEvent = useCallback((event: RunEvent) => {
    setEvents((current) => [...current, event]);
    if (event.event === "report_ready") {
      setStatus("completed");
    } else if (event.event === "run_failed") {
      setStatus("failed");
    } else {
      setStatus("running");
    }
  }, []);

  return { events, status, reset, appendEvent };
}
