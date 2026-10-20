// Codex SDK (in the run image only).

import { type Agent, type AgentInput, emit, instructions, request, say, tool } from "./common.ts";

interface CodexSdk {
  Codex: new (
    options: Record<string, unknown>,
  ) => {
    startThread(options: Record<string, unknown>): {
      runStreamed(
        input: unknown,
        options: { signal: AbortSignal },
      ): Promise<{ events: AsyncIterable<CodexEvent> }>;
    };
  };
}

interface CodexEvent {
  type: string;
  message?: string;
  error?: { message: string };
  usage?: { input_tokens: number; cached_input_tokens: number; output_tokens: number };
  item?: {
    type: string;
    text?: string;
    command?: string;
    changes?: { path: string; kind: string }[];
    message?: string;
  };
}

const SDK = "@openai/codex-sdk";

export const codex: Agent = {
  async run({ spec, work, images, signal }: AgentInput): Promise<void> {
    const sdk = (await import(SDK)) as CodexSdk;
    const env: Record<string, string> = {};
    for (const [key, value] of Object.entries(process.env))
      if (value !== undefined) env[key] = value;
    const client = new sdk.Codex({ apiKey: spec.apiKey, env });
    const thread = client.startThread({
      workingDirectory: work,
      skipGitRepoCheck: true,
      // The run's container is the sandbox.
      sandboxMode: "danger-full-access",
      approvalPolicy: "never",
      networkAccessEnabled: true,
      ...(spec.model ? { model: spec.model } : {}),
    });
    const input = [
      { type: "text", text: `${instructions(spec)}\n\n---\n\n${request(spec)}` },
      ...images.map((image) => ({ type: "local_image", path: image.path })),
    ];
    const { events } = await thread.runStreamed(input, { signal });
    let tokensIn = 0;
    let cachedIn = 0;
    let tokensOut = 0;
    for await (const event of events) {
      if (event.type === "item.completed" && event.item) {
        const item = event.item;
        if (item.type === "agent_message" && item.text) say(item.text);
        if (item.type === "command_execution" && item.command) tool("shell", item.command);
        if (item.type === "file_change") {
          tool("edit", (item.changes ?? []).map((c) => `${c.kind} ${c.path}`).join(", "));
        }
        if (item.type === "error" && item.message) tool("error", item.message);
      }
      if (event.type === "turn.completed" && event.usage) {
        tokensIn += event.usage.input_tokens;
        cachedIn += event.usage.cached_input_tokens;
        tokensOut += event.usage.output_tokens;
        emit({
          type: "usage",
          tokensIn,
          tokensOut,
          cachedIn,
          costUsd: null,
          ...(spec.model ? { model: spec.model } : {}),
        });
      }
      if (event.type === "turn.failed") throw new Error(event.error?.message ?? "the turn failed");
      if (event.type === "error") throw new Error(event.message ?? "codex failed");
    }
  },
};
