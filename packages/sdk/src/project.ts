// The project contract: what a board project declares in `.board/project.ts`.
// Pure data plus functions that only run inside the frame (render, mocks, setup).
// `toManifest` turns it into the JSON the server reads.

/** How one screen leads to another. */
export type ConnectionKind = "click" | "auto" | "error";

export interface Size {
  w: number;
  h: number;
}

/** A route of the app. Screens are states of a page. */
export interface PageDef {
  id: string;
  route: string;
  title: string;
}

/** One state of a page, rendered with mock data. */
export interface ScreenDef<Mock = unknown> {
  id: string;
  page: string;
  title: string;
  note?: string;
  /** Route to open; defaults to the page's route. */
  route?: string;
  /** Viewport; defaults to the project's `screenSize`. */
  size?: Size;
  /** Request handlers for this screen, tried before the project's `mocks`. */
  mocks?: Mock[];
  /** Interactions that bring the screen into its state (click a tab, fill a form). */
  setup?: (kit: SetupKit) => Promise<void>;
}

/** Helpers a screen's `setup` gets. Selectors are CSS; text matches are exact after trimming. */
export interface SetupKit {
  root: HTMLElement;
  find(selector: string, text?: string): Promise<HTMLElement>;
  click(selector: string, text?: string): Promise<void>;
  fill(selector: string, value: string): Promise<void>;
  wait(ms: number): Promise<void>;
}

/** A move from one screen to another (Figma prototype connection). */
export interface ConnectionDef {
  from: string;
  to: string;
  label?: string;
  kind?: ConnectionKind;
}

/** A step of a flow: an existing screen id, or a planned screen that does not exist yet. */
export type FlowStepDef = string | { id: string; title: string; note?: string };

/** The order a user goes through a feature (Figma prototype flow). */
export interface FlowDef {
  id: string;
  title: string;
  steps: FlowStepDef[];
}

/** One state of a component (Storybook meaning), shown in the Components view. */
export interface StoryDef<Node = unknown> {
  id: string;
  /** Component name, e.g. `Button`. Stories of one component are grouped. */
  component: string;
  title: string;
  render: () => Node;
  size?: Size;
}

export interface ProjectDef<Mock = unknown, Node = unknown> {
  title: string;
  pages: PageDef[];
  screens: ScreenDef<Mock>[];
  connections?: ConnectionDef[];
  flows?: FlowDef[];
  stories?: StoryDef<Node>[];
  /** Handlers every screen falls back to. */
  mocks?: Mock[];
  /** Default screen viewport. */
  screenSize?: Size;
}

/** Declares a project. Checks it eagerly so mistakes surface at load time. */
export function defineProject<Mock, Node>(project: ProjectDef<Mock, Node>): ProjectDef<Mock, Node> {
  const problems = validate(project);
  if (problems.length > 0) {
    throw new Error(`invalid board project:\n${problems.map((p) => `  - ${p}`).join("\n")}`);
  }
  return project;
}

export const DEFAULT_SCREEN_SIZE: Size = { w: 1440, h: 900 };
export const DEFAULT_STORY_SIZE: Size = { w: 480, h: 320 };

/** Every problem in `project`, as messages naming the offending id. */
export function validate(project: ProjectDef): string[] {
  const problems: string[] = [];
  const unique = (kind: string, ids: string[]) => {
    const seen = new Set<string>();
    for (const id of ids) {
      if (!/^[a-z0-9][a-z0-9-]*$/.test(id))
        problems.push(`${kind} id "${id}" must be lowercase letters, digits or dashes`);
      if (seen.has(id)) problems.push(`${kind} id "${id}" is used twice`);
      seen.add(id);
    }
    return seen;
  };
  const pages = unique(
    "page",
    project.pages.map((p) => p.id),
  );
  const screens = unique(
    "screen",
    project.screens.map((s) => s.id),
  );
  unique(
    "flow",
    (project.flows ?? []).map((f) => f.id),
  );
  unique(
    "story",
    (project.stories ?? []).map((s) => s.id),
  );
  for (const page of project.pages) {
    if (!page.route.startsWith("/")) problems.push(`page "${page.id}" route must start with /`);
  }
  for (const screen of project.screens) {
    if (!pages.has(screen.page))
      problems.push(`screen "${screen.id}" is on unknown page "${screen.page}"`);
    if (screen.size && !validSize(screen.size))
      problems.push(`screen "${screen.id}" size must be positive`);
  }
  for (const c of project.connections ?? []) {
    for (const end of [c.from, c.to]) {
      if (!screens.has(end))
        problems.push(`connection ${c.from} → ${c.to} names unknown screen "${end}"`);
    }
  }
  for (const flow of project.flows ?? []) {
    if (flow.steps.length === 0) problems.push(`flow "${flow.id}" has no steps`);
    for (const step of flow.steps) {
      // A string that is not a screen is a planned step; planned objects must not shadow screens.
      if (typeof step !== "string" && screens.has(step.id)) {
        problems.push(
          `flow "${flow.id}" plans "${step.id}", which already is a screen; use the id string`,
        );
      }
    }
  }
  if (project.screenSize && !validSize(project.screenSize))
    problems.push("screenSize must be positive");
  return problems;
}

function validSize(size: Size): boolean {
  return size.w > 0 && size.h > 0 && Number.isFinite(size.w) && Number.isFinite(size.h);
}

/** JSON the server reads (`.board/manifest.json`). Functions and handlers are left out. */
export interface Manifest {
  manifestVersion: 1;
  title: string;
  pages: PageDef[];
  screens: {
    id: string;
    page: string;
    title: string;
    note: string;
    route: string;
    size: Size;
    hasSetup: boolean;
  }[];
  connections: Required<ConnectionDef>[];
  flows: {
    id: string;
    title: string;
    steps: { id: string; title: string; note: string; planned: boolean }[];
  }[];
  stories: { id: string; component: string; title: string; size: Size }[];
}

export function toManifest(project: ProjectDef): Manifest {
  const pageRoute = new Map(project.pages.map((p) => [p.id, p.route]));
  const screenTitle = new Map(project.screens.map((s) => [s.id, s.title]));
  const screenSize = project.screenSize ?? DEFAULT_SCREEN_SIZE;
  return {
    manifestVersion: 1,
    title: project.title,
    pages: project.pages.map(({ id, route, title }) => ({ id, route, title })),
    screens: project.screens.map((s) => ({
      id: s.id,
      page: s.page,
      title: s.title,
      note: s.note ?? "",
      route: s.route ?? pageRoute.get(s.page) ?? "/",
      size: s.size ?? screenSize,
      hasSetup: typeof s.setup === "function",
    })),
    connections: (project.connections ?? []).map((c) => ({
      from: c.from,
      to: c.to,
      label: c.label ?? "",
      kind: c.kind ?? "click",
    })),
    flows: (project.flows ?? []).map((f) => ({
      id: f.id,
      title: f.title,
      steps: f.steps.map((step) =>
        typeof step === "string"
          ? {
              id: step,
              title: screenTitle.get(step) ?? step,
              note: "",
              planned: !screenTitle.has(step),
            }
          : { id: step.id, title: step.title, note: step.note ?? "", planned: true },
      ),
    })),
    stories: (project.stories ?? []).map((s) => ({
      id: s.id,
      component: s.component,
      title: s.title,
      size: s.size ?? DEFAULT_STORY_SIZE,
    })),
  };
}
