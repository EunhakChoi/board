// A checkout is a plain folder holding a project's files, plus a private local git repository that
// records the base the files came from. `push` diffs against that base (honouring the project's
// .gitignore, so node_modules and build output never travel).

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

export interface CheckoutState {
  server: string;
  project: string;
  /** Commit of main the files are based on. */
  base: string;
}

const STATE = ".board/checkout.json";

/** Runs git in `dir` with a fixed identity, no signing and no user hooks. */
export function git(dir: string, args: string[], input?: Buffer): Buffer {
  return execFileSync(
    "git",
    [
      "-c",
      "user.name=board",
      "-c",
      "user.email=board@localhost",
      "-c",
      "commit.gpgsign=false",
      "-c",
      "core.hooksPath=/dev/null",
      ...args,
    ],
    { cwd: dir, input, maxBuffer: 1 << 30, stdio: ["pipe", "pipe", "pipe"] },
  );
}

/** Finds the checkout containing `from` (walking up), if any. */
export function findCheckout(from: string): { dir: string; state: CheckoutState } | undefined {
  let dir = resolve(from);
  for (;;) {
    const file = join(dir, STATE);
    if (existsSync(file)) {
      return { dir, state: JSON.parse(readFileSync(file, "utf8")) as CheckoutState };
    }
    const parent = dirname(dir);
    if (parent === dir) return undefined;
    dir = parent;
  }
}

export function writeState(dir: string, state: CheckoutState): void {
  mkdirSync(join(dir, ".board"), { recursive: true });
  writeFileSync(join(dir, STATE), `${JSON.stringify(state, null, 2)}\n`);
}

/** Unpacks a gzipped tar into `dir` and records it as the local base commit. */
export function unpack(dir: string, archive: Buffer): void {
  mkdirSync(dir, { recursive: true });
  execFileSync("tar", ["-xzf", "-", "-C", dir], { input: archive, maxBuffer: 1 << 30 });
}

/** Starts the local base repository after the first unpack. */
export function initBase(dir: string, base: string): void {
  git(dir, ["init", "-q", "-b", "board-base"]);
  // The state file is the checkout's own bookkeeping, never project content.
  writeFileSync(join(dir, ".git", "info", "exclude"), `/${STATE}\n`);
  commitBase(dir, base);
}

/** Records the current files as the base `commit`. */
export function commitBase(dir: string, commit: string): void {
  git(dir, ["add", "-A"]);
  git(dir, ["commit", "-q", "--allow-empty", "--no-verify", "-m", `board base ${commit}`]);
}

export interface LocalChange {
  path: string;
  /** Undefined means the file was deleted. */
  content?: Buffer;
  executable: boolean;
}

/** Files changed since the base, honouring the project's .gitignore. */
export function localChanges(dir: string): LocalChange[] {
  git(dir, ["add", "-A"]);
  const out = git(dir, ["diff", "--cached", "--name-status", "--no-renames", "-z", "HEAD"])
    .toString()
    .split("\0")
    .filter(Boolean);
  const changes: LocalChange[] = [];
  for (let i = 0; i + 1 < out.length; i += 2) {
    const status = out[i] as string;
    const path = out[i + 1] as string;
    if (status.startsWith("D")) {
      changes.push({ path, executable: false });
    } else {
      const file = join(dir, path);
      const executable = (statSync(file).mode & 0o111) !== 0;
      changes.push({ path, content: readFileSync(file), executable });
    }
  }
  return changes;
}
