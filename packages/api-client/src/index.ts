import createFetchClient from "openapi-fetch";
import type { components, paths } from "./schema.d.ts";

export type { components, paths };
export type Schemas = components["schemas"];
export type ApiError = Schemas["Error"];

export interface ClientOptions {
  /** Server origin, e.g. `http://127.0.0.1:5280`. Empty means same origin (board-web). */
  baseUrl?: string;
  /** CLI token, sent as a Bearer header. board-web relies on the session cookie instead. */
  token?: string;
  fetch?: typeof globalThis.fetch;
}

/** Typed client for the board API (`contracts/openapi.yaml`). */
export function createClient({ baseUrl = "", token, fetch }: ClientOptions = {}) {
  return createFetchClient<paths>({
    baseUrl,
    ...(token ? { headers: { authorization: `Bearer ${token}` } } : {}),
    ...(fetch ? { fetch } : {}),
  });
}

export type Client = ReturnType<typeof createClient>;
