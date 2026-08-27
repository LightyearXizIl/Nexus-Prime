import { invoke } from "@tauri-apps/api/core";

export interface AppLogEvent {
  category: string;
  action: string;
  outcome?: string;
  target?: string;
  details?: unknown;
  durationMs?: number;
  sessionId?: string;
}

const queue: AppLogEvent[] = [];
let flushTimer: ReturnType<typeof window.setTimeout> | null = null;
const QUIET_COMMANDS = new Set(["get_xiaomi_voice_meter", "get_xiaomi_host_status", "get_device_status"]);

function flushSoon() {
  if (flushTimer !== null) return;
  flushTimer = window.setTimeout(() => {
    flushTimer = null;
    const events = queue.splice(0, queue.length);
    if (!events.length) return;
    void invoke("append_app_events", { events }).catch(() => {
      // Logging must never break the action that produced the event.
    });
  }, 180);
}

export function recordAppEvent(event: AppLogEvent) {
  if (queue.length >= 500) {
    if (!queue.some((item) => item.action === "queue_overflow")) {
      queue.push({ category: "logging", action: "queue_overflow", outcome: "warning" });
    }
    return;
  }
  queue.push({ outcome: "success", ...event });
  flushSoon();
}

export async function loggedInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const started = performance.now();
  try {
    const result = args === undefined
      ? await invoke<T>(command)
      : await invoke<T>(command, args);
    if (!QUIET_COMMANDS.has(command)) {
      recordAppEvent({
        category: "command",
        action: command,
        durationMs: Math.round(performance.now() - started),
        details: args ?? {},
      });
    }
    return result;
  } catch (error) {
    recordAppEvent({
      category: "command",
      action: command,
      outcome: "error",
      durationMs: Math.round(performance.now() - started),
      details: { args: args ?? {}, error: String(error) },
    });
    throw error;
  }
}

function elementLabel(element: Element): string {
  const named = element.getAttribute("data-log-action")
    || element.getAttribute("aria-label")
    || element.getAttribute("name")
    || element.id;
  if (named) return named.slice(0, 160);
  return (element.textContent || element.tagName).replace(/\s+/g, " ").trim().slice(0, 160);
}

export function installUiLogging() {
  document.addEventListener("click", (event) => {
    const element = (event.target as Element | null)?.closest("button,a,[role=button],[role=tab]");
    if (!element) return;
    recordAppEvent({
      category: "ui",
      action: "click",
      target: elementLabel(element),
      details: { route: window.location.hash },
    });
  }, true);
  document.addEventListener("change", (event) => {
    const element = event.target as HTMLInputElement | HTMLSelectElement | null;
    if (!element || !["INPUT", "SELECT", "TEXTAREA"].includes(element.tagName)) return;
    const value = element instanceof HTMLInputElement && element.type === "checkbox"
      ? element.checked
      : element.value;
    recordAppEvent({
      category: "ui",
      action: "change",
      target: elementLabel(element),
      details: { route: window.location.hash, value },
    });
  }, true);
  window.addEventListener("error", (event) => {
    recordAppEvent({ category: "frontend", action: "window_error", outcome: "error", details: { message: event.message, source: event.filename, line: event.lineno } });
  });
  window.addEventListener("unhandledrejection", (event) => {
    recordAppEvent({ category: "frontend", action: "unhandled_rejection", outcome: "error", details: { reason: String(event.reason) } });
  });
}
