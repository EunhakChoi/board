import createFetchClient from "openapi-fetch";
import type { components, paths } from "./schema.d.ts";

export type { components, paths };
export type Schemas = components["schemas"];
export type ApiError = Schemas["Error"];

export interface ClientOptions {
  /** Server origin, e.g. `http://127.0.0.1:5280`. Empty means same origin (charet-web). */
  baseUrl?: string;
  /** CLI token, sent as a Bearer header. charet-web relies on the session cookie instead. */
  token?: string;
  fetch?: typeof globalThis.fetch;
}

/** Header the server requires on changing requests that rely on the session cookie. */
export const CLIENT_HEADER = "x-charet-client";

/** Typed client for the charet API (`contracts/openapi.yaml`). */
export function createClient({ baseUrl = "", token, fetch }: ClientOptions = {}) {
  return createFetchClient<paths>({
    baseUrl,
    headers: {
      // Changing requests must carry it: cross-site pages cannot send custom headers (CSRF).
      [CLIENT_HEADER]: "charet",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    ...(fetch ? { fetch } : {}),
  });
}

export type Client = ReturnType<typeof createClient>;
