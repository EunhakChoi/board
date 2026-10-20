// What charet-server sends to start a run, what the runner inside a run reports, and what the
// server reads back. The runner talks JSON lines on its stdout.

export type AgentKind = "claude" | "codex" | "script";
export type MessageKind = "user" | "assistant" | "tool" | "status";
export type Finished = "done" | "failed" | "canceled";

/** `POST /v1/runs` from charet-server. */
export interface RunStart {
  id: number;
  agent: AgentKind;
  /** Empty: the agent's default. */
  model: string;
  /** Vendor key; empty for the script agent. */
  apiKey: string;
  project: string;
  branch: number;
  /** For the agent door: good for this run's branch while it runs. */
  token: string;
  prompt: string;
  context: unknown;
  images: { mediaType: string; data: string }[];
  history: { role: "user" | "assistant"; text: string }[];
}

/** What a run container finds in its spec file (`CHARET_RUN_SPEC`). */
export interface RunSpec extends Omit<RunStart, "images" | "token"> {
  /** Image files next to the spec. */
  images: { mediaType: string; file: string }[];
}

export interface RunEvent {
  kind: MessageKind;
  text: string;
  tool?: string;
}

/** One line on the runner's stdout. */
export type RunnerLine =
  | ({ type: "event" } & RunEvent)
  | {
      type: "usage";
      tokensIn: number;
      tokensOut: number;
      /** Reported by the agent; `null`: work it out from the price table. */
      costUsd: number | null;
      model?: string;
      cachedIn?: number;
    }
  | { type: "result"; state: Finished; error?: string };

/** `GET /v1/runs/{id}?after=N` to charet-server. */
export interface Progress {
  events: RunEvent[];
  next: number;
  finished: Finished | null;
  error: string | null;
  tokensIn: number;
  tokensOut: number;
  costUsd: number | null;
}

export function isRunStart(value: unknown): value is RunStart {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    Number.isSafeInteger(v.id) &&
    (v.agent === "claude" || v.agent === "codex" || v.agent === "script") &&
    typeof v.model === "string" &&
    typeof v.apiKey === "string" &&
    typeof v.project === "string" &&
    /^[a-z0-9][a-z0-9-]{0,62}$/.test(v.project) &&
    Number.isSafeInteger(v.branch) &&
    typeof v.token === "string" &&
    /^[0-9a-f]{16,128}$/.test(v.token) &&
    typeof v.prompt === "string" &&
    Array.isArray(v.images) &&
    v.images.every(
      (i: unknown) =>
        typeof i === "object" &&
        i !== null &&
        typeof (i as Record<string, unknown>).mediaType === "string" &&
        typeof (i as Record<string, unknown>).data === "string",
    ) &&
    Array.isArray(v.history)
  );
}
