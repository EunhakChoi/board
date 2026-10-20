// Server state for the web tool (TanStack Query over the typed API client).

import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  api,
  type Comment,
  HttpError,
  type Role,
  type ShareMode,
  type TargetKind,
  type Theme,
  unwrap,
  unwrapEmpty,
} from "./api.ts";

const path = (key: string) => ({ params: { path: { key } } });

export function useProjects() {
  return useQuery({ queryKey: ["projects"], queryFn: () => unwrap(api.GET("/api/projects")) });
}

export function useProject(key: string) {
  return useQuery({
    queryKey: ["project", key],
    queryFn: () => unwrap(api.GET("/api/projects/{key}", path(key))),
  });
}

export function useVersions(key: string) {
  return useQuery({
    queryKey: ["versions", key],
    queryFn: () => unwrap(api.GET("/api/projects/{key}/versions", path(key))),
  });
}

/** The preview of `main` (or of `rev`); polls while it builds. */
export function usePreview(key: string, rev?: string) {
  return useQuery({
    queryKey: ["preview", key, rev ?? "main"],
    queryFn: () =>
      unwrap(
        api.GET("/api/projects/{key}/preview", {
          params: { path: { key }, query: rev ? { rev } : {} },
        }),
      ),
    refetchInterval: (query) => {
      const state = query.state.data?.state;
      return state === "queued" || state === "running" ? 1500 : false;
    },
  });
}

/** Pages, screens, flows and stories of `main` (or of `rev`); retried while the preview builds. */
export function useManifest(key: string, rev?: string) {
  return useQuery({
    queryKey: ["manifest", key, rev ?? "main"],
    queryFn: () =>
      unwrap(
        api.GET("/api/projects/{key}/manifest", {
          params: { path: { key }, query: rev ? { rev } : {} },
        }),
      ),
    retry: (count, error) => error instanceof HttpError && error.code === "pending" && count < 400,
    retryDelay: 1500,
    placeholderData: keepPreviousData,
  });
}

export function useCaptures(key: string) {
  return useQuery({
    queryKey: ["captures", key],
    queryFn: () => unwrap(api.GET("/api/projects/{key}/captures", path(key))),
  });
}

export function useElements(
  key: string,
  kind: TargetKind,
  target: string,
  theme: Theme,
  enabled: boolean,
) {
  return useQuery({
    queryKey: ["elements", key, kind, target, theme],
    queryFn: () =>
      unwrap(
        api.GET("/api/projects/{key}/captures/{kind}/{target}/{theme}/elements", {
          params: { path: { key, kind, target, theme } },
        }),
      ),
    enabled,
  });
}

export function useComments(key: string) {
  return useQuery({
    queryKey: ["comments", key],
    queryFn: () => unwrap(api.GET("/api/projects/{key}/comments", path(key))),
  });
}

export function useFiles(key: string) {
  return useQuery({
    queryKey: ["files", key],
    queryFn: () => unwrap(api.GET("/api/projects/{key}/files", path(key))),
  });
}

/** A file of `main` as text (`undefined` when it is not UTF-8). */
export function useFile(key: string, file: string | undefined) {
  return useQuery({
    queryKey: ["file", key, file],
    enabled: !!file,
    retry: (count, error) => !(error instanceof HttpError && error.status < 500) && count < 3,
    queryFn: async () => {
      const response = await fetch(
        `/api/projects/${encodeURIComponent(key)}/file?path=${encodeURIComponent(file ?? "")}`,
      );
      if (!response.ok) {
        const body = (await response.json().catch(() => ({}))) as {
          code?: string;
          message?: string;
        };
        throw new HttpError(
          response.status,
          body.code ?? "error",
          body.message ?? response.statusText,
        );
      }
      const text = response.headers.get("content-type")?.startsWith("text/")
        ? await response.text()
        : undefined;
      return { text };
    },
  });
}

/** The app directory inside the repository (`charet.json` `app`), `""` for the root. */
export function useAppDir(key: string) {
  const file = useFile(key, "charet.json");
  if (!file.data?.text) return "";
  try {
    const app = (JSON.parse(file.data.text) as { app?: unknown }).app;
    return typeof app === "string" && app !== "." ? app : "";
  } catch {
    return "";
  }
}

export function useCapture(key: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: { targets?: string[]; themes?: Theme[]; force?: boolean }) =>
      unwrap(api.POST("/api/projects/{key}/captures", { ...path(key), body: input })),
    onSettled: () => {
      void client.invalidateQueries({ queryKey: ["captures", key] });
      void client.invalidateQueries({ queryKey: ["elements", key] });
      void client.invalidateQueries({ queryKey: ["comments", key] });
    },
  });
}

export function useRetryBuild(key: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () =>
      unwrap(
        api.GET("/api/projects/{key}/preview", {
          params: { path: { key }, query: { retry: true } },
        }),
      ),
    onSuccess: (preview) => {
      client.setQueryData(["preview", key], preview);
      void client.invalidateQueries({ queryKey: ["manifest", key] });
    },
  });
}

export type NewComment = {
  kind: TargetKind;
  target: string;
  theme: Theme;
  at: { x: number; y: number } | { selector: string };
  body: string;
  /** On this branch's screen instead of main's. */
  branch?: number;
};

/** Comments on a branch's screens. */
export function useBranchComments(key: string, number: number) {
  return useQuery({
    queryKey: ["comments", key, "branch", number],
    queryFn: () =>
      unwrap(
        api.GET("/api/projects/{key}/comments", {
          params: { path: { key }, query: { branch: number } },
        }),
      ),
  });
}

export function useAddComment(key: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: NewComment) =>
      unwrap(api.POST("/api/projects/{key}/comments", { ...path(key), body: input })),
    onSuccess: () => client.invalidateQueries({ queryKey: ["comments", key] }),
  });
}

export function useAnswer(key: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: async (input: {
      comment: Comment;
      action: "replies" | "resolve" | "reopen";
      body: string;
    }) => {
      const params = {
        params: { path: { key, id: input.comment.id } },
        body: { rev: input.comment.rev, body: input.body },
      };
      switch (input.action) {
        case "replies":
          return unwrap(api.POST("/api/projects/{key}/comments/{id}/replies", params));
        case "resolve":
          return unwrap(api.POST("/api/projects/{key}/comments/{id}/resolve", params));
        case "reopen":
          return unwrap(api.POST("/api/projects/{key}/comments/{id}/reopen", params));
      }
    },
    // A stale write (`changed`) also means the list is out of date.
    onSettled: () => client.invalidateQueries({ queryKey: ["comments", key] }),
  });
}

/** Who is looking: this machine's admin, a member, or a visitor. */
export function useMe() {
  return useQuery({
    queryKey: ["me"],
    retry: (count, error) => !(error instanceof HttpError && error.status < 500) && count < 3,
    queryFn: () => unwrap(api.GET("/api/auth/me")),
  });
}

/** Comes in with a member's token; a fresh page load picks it up everywhere. */
export function useEnterToken() {
  return useMutation({
    mutationFn: (token: string) => unwrap(api.POST("/api/auth/token", { body: { token } })),
  });
}

export function useSignOut() {
  return useMutation({
    mutationFn: () => unwrapEmpty(api.POST("/api/auth/logout")).catch(() => undefined),
    onSettled: () => window.location.assign("/"),
  });
}

/** Members with their tokens, for the admin's Team page. */
export function useUsers(enabled = true) {
  return useQuery({
    queryKey: ["admin", "users"],
    queryFn: () => unwrap(api.GET("/api/admin/users")),
    enabled,
    refetchInterval: 10_000,
  });
}

export function useAccess(enabled: boolean) {
  return useQuery({
    queryKey: ["admin", "access"],
    queryFn: () => unwrap(api.GET("/api/admin/access")),
    enabled,
  });
}

export function useAdminActions() {
  const client = useQueryClient();
  const settled = () => client.invalidateQueries({ queryKey: ["admin"] });
  const user = (id: string) => ({ params: { path: { id } } });
  return {
    add: useMutation({
      mutationFn: (body: { name: string; role: Role }) =>
        unwrap(api.POST("/api/admin/users", { body })),
      onSettled: settled,
    }),
    change: useMutation({
      mutationFn: ({ id, ...body }: { id: string; name?: string; role?: Role }) =>
        unwrap(api.PATCH("/api/admin/users/{id}", { ...user(id), body })),
      onSettled: () => {
        settled();
        // Renaming the owner renames this machine's admin.
        client.invalidateQueries({ queryKey: ["me"] });
      },
    }),
    reissue: useMutation({
      mutationFn: (id: string) => unwrap(api.POST("/api/admin/users/{id}/token", user(id))),
      onSettled: settled,
    }),
    remove: useMutation({
      mutationFn: (id: string) => unwrapEmpty(api.DELETE("/api/admin/users/{id}", user(id))),
      onSettled: settled,
    }),
    visitors: useMutation({
      mutationFn: (visitors: boolean) =>
        unwrap(api.PUT("/api/admin/access", { body: { visitors } })),
      onSettled: settled,
    }),
  };
}

export function useProjectAccess(key: string, enabled: boolean) {
  return useQuery({
    queryKey: ["members", key],
    enabled,
    queryFn: () => unwrap(api.GET("/api/projects/{key}/members", path(key))),
  });
}

export function useProjectAccessActions(key: string) {
  const client = useQueryClient();
  return {
    visibility: useMutation({
      mutationFn: (visibility: "team" | "private") =>
        unwrap(api.PATCH("/api/projects/{key}", { ...path(key), body: { visibility } })),
      onSettled: () => {
        client.invalidateQueries({ queryKey: ["project", key] });
        client.invalidateQueries({ queryKey: ["projects"] });
      },
    }),
    member: useMutation({
      mutationFn: ({ user, member }: { user: string; member: boolean }) => {
        const params = { params: { path: { key, user } } };
        return unwrapEmpty(
          member
            ? api.PUT("/api/projects/{key}/members/{user}", params)
            : api.DELETE("/api/projects/{key}/members/{user}", params),
        );
      },
      // The checkbox follows the click at once; the refetch confirms it.
      onMutate: async ({ user, member }) => {
        // A fetch still in flight would bring back the old list.
        await client.cancelQueries({ queryKey: ["members", key] });
        client.setQueryData<{ members: string[] }>(["members", key], (old) => {
          const rest = (old?.members ?? []).filter((id) => id !== user);
          return { members: member ? [...rest, user] : rest };
        });
      },
      onSettled: () => client.invalidateQueries({ queryKey: ["members", key] }),
    }),
  };
}

/** The signed-in member's inbox. Realtime nudges refetch it; the poll covers pages without a room. */
export function useInbox(enabled: boolean) {
  return useQuery({
    queryKey: ["notifications"],
    enabled,
    queryFn: () => unwrap(api.GET("/api/me/notifications", { params: { query: { limit: 30 } } })),
    refetchInterval: 60_000,
  });
}

export function useMarkRead() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (ids?: number[]) =>
      unwrap(api.POST("/api/me/notifications/read", { body: ids ? { ids } : {} })),
    onSettled: () => client.invalidateQueries({ queryKey: ["notifications"] }),
  });
}

export function useWebhooks() {
  return useQuery({
    queryKey: ["admin", "webhooks"],
    queryFn: () => unwrap(api.GET("/api/admin/webhooks")),
  });
}

export function useWebhookActions() {
  const client = useQueryClient();
  const settled = () => client.invalidateQueries({ queryKey: ["admin", "webhooks"] });
  const hook = (id: string) => ({ params: { path: { id } } });
  return {
    create: useMutation({
      mutationFn: (body: { url: string; events: string[]; project?: string | null }) =>
        unwrap(api.POST("/api/admin/webhooks", { body })),
      onSettled: settled,
    }),
    remove: useMutation({
      mutationFn: (id: string) => unwrapEmpty(api.DELETE("/api/admin/webhooks/{id}", hook(id))),
      onSettled: settled,
    }),
    rotate: useMutation({
      mutationFn: (id: string) => unwrap(api.POST("/api/admin/webhooks/{id}/rotate", hook(id))),
      onSettled: settled,
    }),
    test: useMutation({
      mutationFn: (id: string) => unwrap(api.POST("/api/admin/webhooks/{id}/test", hook(id))),
      onSettled: settled,
    }),
  };
}

/** Versions, links and services (answers without a token). */
export function useSystem() {
  return useQuery({
    queryKey: ["system"],
    queryFn: () => unwrap(api.GET("/api/system")),
    staleTime: 60_000,
  });
}

export function useSharing(enabled: boolean) {
  return useQuery({
    queryKey: ["admin", "sharing"],
    queryFn: () => unwrap(api.GET("/api/admin/sharing")),
    enabled,
  });
}

export function useSharingActions() {
  const client = useQueryClient();
  const settled = () => {
    client.invalidateQueries({ queryKey: ["admin", "sharing"] });
    client.invalidateQueries({ queryKey: ["system"] });
  };
  return {
    start: useMutation({
      mutationFn: (body: { mode: ShareMode; url?: string; authtoken?: string; domain?: string }) =>
        unwrap(api.PUT("/api/admin/sharing", { body })),
      onSettled: settled,
    }),
    stop: useMutation({
      mutationFn: () => unwrap(api.DELETE("/api/admin/sharing")),
      onSettled: settled,
    }),
  };
}

export function useBranches(key: string) {
  return useQuery({
    queryKey: ["branches", key],
    queryFn: () => unwrap(api.GET("/api/projects/{key}/branches", path(key))),
  });
}

export function useBranch(key: string, number: number) {
  return useQuery({
    queryKey: ["branch", key, number],
    queryFn: () =>
      unwrap(
        api.GET("/api/projects/{key}/branches/{number}", {
          params: { path: { key, number } },
        }),
      ),
  });
}

/** A file at `rev` as text (`undefined` when missing or not UTF-8). */
export function useFileAt(key: string, rev: string | undefined, file: string | undefined) {
  return useQuery({
    queryKey: ["file", key, rev, file],
    enabled: !!rev && !!file,
    queryFn: async () => {
      const response = await fetch(
        `/api/projects/${encodeURIComponent(key)}/file?rev=${encodeURIComponent(rev ?? "")}&path=${encodeURIComponent(file ?? "")}`,
      );
      if (response.status === 404) return { text: undefined };
      if (!response.ok) throw new HttpError(response.status, "error", response.statusText);
      const text = response.headers.get("content-type")?.startsWith("text/")
        ? await response.text()
        : undefined;
      return { text };
    },
  });
}

/** Review, merge, archive, reopen, duplicate and catch-up of branches; restoring versions. */
export function useBranchActions(key: string) {
  const client = useQueryClient();
  const settled = () => {
    void client.invalidateQueries({ queryKey: ["branches", key] });
    void client.invalidateQueries({ queryKey: ["branch", key] });
    void client.invalidateQueries({ queryKey: ["versions", key] });
  };
  const on = (number: number) => ({ params: { path: { key, number } } });
  return {
    review: useMutation({
      mutationFn: (number: number) =>
        unwrap(api.POST("/api/projects/{key}/branches/{number}/request-review", on(number))),
      onSettled: settled,
    }),
    merge: useMutation({
      mutationFn: (number: number) =>
        unwrap(api.POST("/api/projects/{key}/branches/{number}/merge", on(number))),
      onSettled: settled,
    }),
    archive: useMutation({
      mutationFn: (number: number) =>
        unwrap(api.POST("/api/projects/{key}/branches/{number}/archive", on(number))),
      onSettled: settled,
    }),
    reopen: useMutation({
      mutationFn: (number: number) =>
        unwrap(api.POST("/api/projects/{key}/branches/{number}/reopen", on(number))),
      onSettled: settled,
    }),
    duplicate: useMutation({
      mutationFn: (number: number) =>
        unwrap(api.POST("/api/projects/{key}/branches/{number}/duplicate", on(number))),
      onSettled: settled,
    }),
    catchUp: useMutation({
      mutationFn: (number: number) =>
        unwrap(api.POST("/api/projects/{key}/branches/{number}/catch-up", on(number))),
      onSettled: settled,
    }),
    restore: useMutation({
      mutationFn: (number: number) =>
        unwrap(
          api.POST("/api/projects/{key}/versions/{number}/restore", {
            params: { path: { key, number } },
          }),
        ),
      onSettled: settled,
    }),
  };
}

/** Paths changed from `from` to `to` (each `main`, `v<n>`, `b<n>` or a commit). */
export function useDiff(key: string, from: string | undefined, to: string | undefined) {
  return useQuery({
    queryKey: ["diff", key, from, to],
    enabled: !!from && !!to,
    queryFn: () =>
      unwrap(
        api.GET("/api/projects/{key}/diff", {
          params: { path: { key }, query: { from: from ?? "", to: to ?? "" } },
        }),
      ),
  });
}
