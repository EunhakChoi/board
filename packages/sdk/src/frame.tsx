// The frame renders one screen or one story of a board project, with mock APIs, for the board to
// show and capture. URL: `.board/frame.html?screen=<id>&theme=light|dark` or `?story=<id>`.
// When the page is settled it sets `<html data-board-ready>`; the renderer waits for that.

import type { RequestHandler } from "msw";
import { setupWorker } from "msw/browser";
import { type ReactNode, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import type { ProjectDef, ScreenDef, SetupKit } from "./project.ts";

export interface FrameOptions {
  project: ProjectDef<RequestHandler, ReactNode>;
  /** Renders the app at `route` (e.g. a memory router over the app's routes, inside its providers). */
  renderScreen: (screen: ScreenDef<RequestHandler>, route: string) => ReactNode;
  /** Wraps stories, e.g. with the app's theme provider. */
  wrapStory?: (node: ReactNode) => ReactNode;
  /** Root element id (default `root`). */
  rootId?: string;
}

/** Logged when a screen calls an API with no handler; the board reports it as a missing mock. */
export const MISSING_MOCK = "[board] missing mock";

export async function startFrame(options: FrameOptions): Promise<void> {
  const params = new URLSearchParams(location.search);
  const theme = params.get("theme") === "dark" ? "dark" : "light";
  document.documentElement.classList.toggle("dark", theme === "dark");
  document.documentElement.dataset.theme = theme;

  const container = document.getElementById(options.rootId ?? "root");
  if (!container) throw new Error(`no #${options.rootId ?? "root"} element`);
  const { project } = options;
  const screenId = params.get("screen");
  const storyId = params.get("story");

  let node: ReactNode;
  let screen: ScreenDef<RequestHandler> | undefined;
  if (screenId) {
    screen = project.screens.find((s) => s.id === screenId);
    if (!screen) return fail(container, `unknown screen "${screenId}"`);
    const page = project.pages.find((p) => p.id === screen?.page);
    const handlers = [...(screen.mocks ?? []), ...(project.mocks ?? [])];
    const worker = setupWorker(...handlers);
    await worker.start({
      quiet: true,
      onUnhandledRequest(request, print) {
        const url = new URL(request.url);
        // Only API calls count; the page's own assets are not mocks.
        if (url.origin === location.origin && /\.\w+$/.test(url.pathname)) return;
        console.warn(`${MISSING_MOCK} ${request.method} ${url.pathname}${url.search}`);
        print.warning();
      },
    });
    node = options.renderScreen(screen, screen.route ?? page?.route ?? "/");
  } else if (storyId) {
    const story = project.stories?.find((s) => s.id === storyId);
    if (!story) return fail(container, `unknown story "${storyId}"`);
    node = (options.wrapStory ?? ((n) => n))(story.render());
  } else {
    return fail(container, "pass ?screen=<id> or ?story=<id>");
  }

  createRoot(container).render(<StrictMode>{node}</StrictMode>);
  await settle();
  if (screen?.setup) {
    await screen.setup(setupKit(container));
    await settle();
  }
  document.documentElement.dataset.boardReady = "true";
}

function fail(container: HTMLElement, message: string): void {
  container.textContent = `board: ${message}`;
  console.error(`[board] ${message}`);
  document.documentElement.dataset.boardReady = "error";
}

/** Waits for two frames and for fonts, so the first paint after data arrives is in. */
async function settle(): Promise<void> {
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  await document.fonts?.ready;
  await new Promise((r) => setTimeout(r, 50));
}

function setupKit(root: HTMLElement): SetupKit {
  const find = async (selector: string, text?: string): Promise<HTMLElement> => {
    for (let i = 0; i < 100; i++) {
      const match = [...root.querySelectorAll<HTMLElement>(selector)].find(
        (el) => text === undefined || el.textContent?.trim() === text,
      );
      if (match) return match;
      await new Promise((r) => setTimeout(r, 50));
    }
    throw new Error(`setup: no ${selector}${text ? ` with text "${text}"` : ""}`);
  };
  return {
    root,
    find,
    async click(selector, text) {
      (await find(selector, text)).click();
    },
    async fill(selector, value) {
      const input = (await find(selector)) as HTMLInputElement;
      const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), "value")?.set;
      setter?.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    },
    wait: (ms) => new Promise((r) => setTimeout(r, ms)),
  };
}
