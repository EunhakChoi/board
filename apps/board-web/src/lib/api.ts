import { createClient, type Schemas } from "@board/api-client";

/** Same-origin client: board-server serves the web tool next to the API. */
export const api = createClient();

export type Manifest = Schemas["Manifest"];
export type Capture = Schemas["Capture"];
export type Comment = Schemas["Comment"];
export type Anchor = Schemas["Anchor"];
export type Element = Schemas["Element"];
export type Preview = Schemas["Preview"];
export type Version = Schemas["Version"];
export type Project = Schemas["Project"];
export type Theme = "light" | "dark";
export type TargetKind = "screen" | "story";

/** An API error with its code (`pending`, `changed`, …). */
export class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly detail?: unknown,
  ) {
    super(message);
  }
}

/** The data of an openapi-fetch result, or its error thrown as an [`HttpError`]. */
export async function unwrap<T>(
  call: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<T> {
  const { data, error, response } = await call;
  if (error !== undefined || data === undefined) {
    const body = (error ?? {}) as { code?: string; message?: string; detail?: unknown };
    throw new HttpError(
      response.status,
      body.code ?? "error",
      body.message ?? response.statusText,
      body.detail,
    );
  }
  return data;
}

export function captureImage(
  key: string,
  capture: Pick<Capture, "kind" | "target" | "theme" | "capturedAt">,
) {
  const path = `/api/projects/${encodeURIComponent(key)}/captures/${capture.kind}/${encodeURIComponent(capture.target)}/${capture.theme}/image`;
  return `${path}?v=${encodeURIComponent(capture.capturedAt)}`;
}

export function snapshotImage(key: string, comment: Pick<Comment, "id">) {
  return `/api/projects/${encodeURIComponent(key)}/comments/${comment.id}/snapshot`;
}
