// Server state for the web tool (TanStack Query over the typed API client).

import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  api,
  type Comment,
  HttpError,
  type Role,
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

/** The preview of `main`; polls while it builds. */
export function usePreview(key: string) {
  return useQuery({
    queryKey: ["preview", key],
    queryFn: () => unwrap(api.GET("/api/projects/{key}/preview", path(key))),
    refetchInterval: (query) => {
      const state = query.state.data?.state;
      return state === "queued" || state === "running" ? 1500 : false;
    },
  });
}

/** Pages, screens, flows and stories of `main`; retried while the preview builds. */
export function useManifest(key: string) {
  return useQuery({
    queryKey: ["manifest", key],
    queryFn: () => unwrap(api.GET("/api/projects/{key}/manifest", path(key))),
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

/** The app directory inside the repository (`board.json` `app`), `""` for the root. */
export function useAppDir(key: string) {
  const file = useFile(key, "board.json");
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
};

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

/** Who is looking: the local admin, a member, a guest, or someone waiting for approval. */
export function useMe() {
  return useQuery({
    queryKey: ["me"],
    retry: (count, error) => !(error instanceof HttpError && error.status < 500) && count < 3,
    queryFn: () => unwrap(api.GET("/api/auth/me")),
    // Someone waiting for approval is let in as soon as an admin approves.
    refetchInterval: (query) => (query.state.data?.kind === "pending" ? 5_000 : false),
  });
}

export function useSignOut() {
  return useMutation({
    mutationFn: () => unwrapEmpty(api.POST("/api/auth/logout")).catch(() => undefined),
    onSettled: () => window.location.assign("/signin"),
  });
}

/** Accounts and the allowlist, for the admin's Team page. Polled so new requests show up. */
export function useUsers() {
  return useQuery({
    queryKey: ["admin", "users"],
    queryFn: () => unwrap(api.GET("/api/admin/users")),
    refetchInterval: 10_000,
  });
}

export function useAllowRules() {
  return useQuery({
    queryKey: ["admin", "allow-rules"],
    queryFn: () => unwrap(api.GET("/api/admin/allow-rules")),
  });
}

export function useAdminActions() {
  const client = useQueryClient();
  const settled = () => client.invalidateQueries({ queryKey: ["admin"] });
  const user = (id: string) => ({ params: { path: { id } } });
  return {
    approve: useMutation({
      mutationFn: (id: string) => unwrap(api.POST("/api/admin/users/{id}/approve", user(id))),
      onSettled: settled,
    }),
    reject: useMutation({
      mutationFn: (id: string) => unwrap(api.POST("/api/admin/users/{id}/reject", user(id))),
      onSettled: settled,
    }),
    change: useMutation({
      mutationFn: ({
        id,
        ...body
      }: {
        id: string;
        role?: Role;
        status?: "active" | "rejected" | "removed";
      }) => unwrap(api.PATCH("/api/admin/users/{id}", { ...user(id), body })),
      onSettled: settled,
    }),
    addRule: useMutation({
      mutationFn: (body: { kind: "domain" | "email"; value: string }) =>
        unwrap(api.POST("/api/admin/allow-rules", { body })),
      onSettled: settled,
    }),
    removeRule: useMutation({
      mutationFn: (id: number) =>
        unwrapEmpty(api.DELETE("/api/admin/allow-rules/{id}", { params: { path: { id } } })),
      onSettled: settled,
    }),
  };
}

export function useShareLinks(key: string, enabled: boolean) {
  return useQuery({
    queryKey: ["share-links", key],
    enabled,
    queryFn: () => unwrap(api.GET("/api/projects/{key}/share-links", path(key))),
  });
}

export function useShareLinkActions(key: string) {
  const client = useQueryClient();
  const settled = () => client.invalidateQueries({ queryKey: ["share-links", key] });
  return {
    create: useMutation({
      mutationFn: (body: { role: "editor" | "commenter" | "viewer"; label: string }) =>
        unwrap(api.POST("/api/projects/{key}/share-links", { ...path(key), body })),
      onSettled: settled,
    }),
    revoke: useMutation({
      mutationFn: (id: string) =>
        unwrapEmpty(
          api.POST("/api/projects/{key}/share-links/{id}/revoke", {
            params: { path: { key, id } },
          }),
        ),
      onSettled: settled,
    }),
  };
}

/** What a share link opens, for the name page. */
export function useSharedLink(token: string) {
  return useQuery({
    queryKey: ["shared-link", token],
    retry: false,
    queryFn: () => unwrap(api.GET("/api/links/{token}", { params: { path: { token } } })),
  });
}

export function useEnterLink(token: string) {
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api.POST("/api/links/{token}/enter", { params: { path: { token } }, body: { name } })),
  });
}

export function useDeviceLogin(code: string) {
  return useQuery({
    queryKey: ["device", code],
    enabled: code.length > 0,
    retry: false,
    queryFn: () => unwrap(api.GET("/api/auth/device/{code}", { params: { path: { code } } })),
  });
}

export function useDecideDevice(code: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (approve: boolean) =>
      unwrapEmpty(
        api.POST(approve ? "/api/auth/device/{code}/approve" : "/api/auth/device/{code}/deny", {
          params: { path: { code } },
        }),
      ),
    onSettled: () => client.invalidateQueries({ queryKey: ["device", code] }),
  });
}

export function useCliTokens() {
  return useQuery({
    queryKey: ["cli-tokens"],
    queryFn: () => unwrap(api.GET("/api/me/cli-tokens")),
  });
}

export function useCliTokenActions() {
  const client = useQueryClient();
  const settled = () => client.invalidateQueries({ queryKey: ["cli-tokens"] });
  return {
    create: useMutation({
      mutationFn: (name: string) => unwrap(api.POST("/api/me/cli-tokens", { body: { name } })),
      onSettled: settled,
    }),
    revoke: useMutation({
      mutationFn: (id: string) =>
        unwrapEmpty(api.DELETE("/api/me/cli-tokens/{id}", { params: { path: { id } } })),
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
      onMutate: ({ user, member }) => {
        client.setQueryData<{ members: string[] }>(["members", key], (old) => {
          const rest = (old?.members ?? []).filter((id) => id !== user);
          return { members: member ? [...rest, user] : rest };
        });
      },
      onSettled: () => client.invalidateQueries({ queryKey: ["members", key] }),
    }),
  };
}
