// A scripted stand-in for tests and e2e: no model, no key. Each prompt line is a step:
//   write <path>: <text>   writes a file in the checkout
//   say <text>             answers
//   wait <ms>              sleeps (to test cancel)
//   fail <message>         fails the run
// Any other prompt writes it to `charet-agent-note.txt`.

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, normalize } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { type Agent, type AgentInput, cli, emit, say, tool } from "./common.ts";

export const script: Agent = {
  async run({ spec, work, signal }: AgentInput): Promise<void> {
    const steps = spec.prompt
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    let acted = false;
    for (const step of steps) {
      signal.throwIfAborted();
      const write = /^write\s+([^:]+):\s?(.*)$/.exec(step);
      if (write?.[1] !== undefined) {
        const path = normalize(write[1].trim());
        if (path.startsWith("..") || path.startsWith("/"))
          throw new Error(`not in the checkout: ${path}`);
        mkdirSync(dirname(join(work, path)), { recursive: true });
        writeFileSync(join(work, path), `${(write[2] ?? "").replaceAll("\\n", "\n")}\n`);
        tool("Write", path);
        const pushed = await cli(["push", "-m", `write ${path}`], work);
        tool("Bash", `charet push -m "write ${path}"`);
        if (pushed.code !== 0) throw new Error(pushed.stderr.trim() || "push failed");
        acted = true;
      } else if (step.startsWith("say ")) {
        say(step.slice(4));
        acted = true;
      } else if (step.startsWith("wait ")) {
        await sleep(Number(step.slice(5)) || 0, undefined, { signal });
        acted = true;
      } else if (step.startsWith("fail")) {
        throw new Error(step.slice(4).trim() || "asked to fail");
      }
    }
    if (!acted) {
      writeFileSync(join(work, "charet-agent-note.txt"), `${spec.prompt}\n`);
      tool("Write", "charet-agent-note.txt");
      say("Wrote the request to charet-agent-note.txt.");
    }
    emit({ type: "usage", tokensIn: 0, tokensOut: 0, costUsd: 0 });
  },
};
