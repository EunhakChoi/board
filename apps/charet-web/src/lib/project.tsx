import { createContext, useContext, useMemo } from "react";
import type { Capture, Comment, Manifest, TargetKind, Theme } from "./api.ts";
import { useCaptures, useComments, useManifest } from "./queries.ts";

/** What every project page needs: the manifest, captures and comments of `main`. */
export interface ProjectData {
  key: string;
  manifest: Manifest | undefined;
  captureOf: (kind: TargetKind, target: string, theme: Theme) => Capture | undefined;
  commentsOn: (kind: TargetKind, target: string) => Comment[];
  staleCount: number;
}

const Context = createContext<ProjectData | null>(null);

export function useProjectData(key: string): ProjectData {
  const manifest = useManifest(key);
  const captures = useCaptures(key);
  const comments = useComments(key);
  return useMemo(() => {
    const byTarget = new Map<string, Capture>();
    for (const capture of captures.data?.captures ?? []) {
      byTarget.set(`${capture.kind}:${capture.target}:${capture.theme}`, capture);
    }
    const all = comments.data?.comments ?? [];
    return {
      key,
      manifest: manifest.data?.manifest,
      captureOf: (kind, target, theme) => byTarget.get(`${kind}:${target}:${theme}`),
      commentsOn: (kind, target) => all.filter((c) => c.kind === kind && c.target === target),
      staleCount: (captures.data?.captures ?? []).filter((c) => c.stale).length,
    };
  }, [key, manifest.data, captures.data, comments.data]);
}

export const ProjectContext = Context.Provider;

export function useProjectContext(): ProjectData {
  const value = useContext(Context);
  if (!value) throw new Error("useProjectContext outside a project page");
  return value;
}
