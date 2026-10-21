# Hand edits

**English** | [한국어](hand-edits.ko.md)

On a screen, the **Design** panel (editors) changes the code behind an element by hand: its text,
a variant, its spacing, its place among its siblings. Each change is one exact edit of the source
on your draft, and the draft goes to review like any other ([workflow](workflow.md)).

## Using it

1. Open a screen and pick **Design** in the side panel. *Work on* picks one of your drafts or
   **A new draft** (it starts with your first change, named after the screen).
2. Click an element on the screen. The panel shows what can change there:
   - **Text** — the element's own text, written out in the code.
   - **variant**, **size**, … — the `cva` variants of a component (shadcn's `Button`, `Badge`).
   - **Spacing** — Tailwind padding, margin and gap classes (`p-4`, `gap-x-2`, …), or the whole
     **Classes** list. A component with a `size` variant sets its padding through it.
   - **Order** — swap with the element before or after it.
3. Every change is saved on the draft and **checked** like a merge: the draft builds and the
   screens it can reach open without new problems. One that breaks a screen is **taken back**,
   with the reason. While you work, the screen shows the draft (rendered on demand), not `main`.
4. **Undo** / **Redo** step through your changes in this session (as long as nobody changed the
   file since). **Request review** when it is ready.

## What it does not change

A value the code computes — text from data, `variant={on ? "default" : "outline"}`, classes built
with `cn(...)`, children that come from a list — is not a hand edit: the panel says so and
**Ask the agent** hands it to the [web agent](agent.md). Free layout (dragging things around) is
not supported. Inside a list (`.map(...)`) a change applies to every item; the panel warns.

A component used on the screen is edited where it is used (**This screen**). When the element is
written inside a shared component, **Component** edits that file instead and shows how many
screens render from it.

## How it works

- `@charet/sdk/vite` marks host elements with `data-src` and component instances with
  `data-charet-use` (`path:line:col`). The element map of a render keeps both.
- The server reads the file at the draft's head, parses it (oxc) and changes one place by a fixed
  rule (`crates/design`); the result must still parse. The edit is one commit on the draft.
- The check reuses the merge check: build, then the screens the change reaches, compared with
  `main`'s captures. A failure pushes the file back as it was.

Not yet: project type checks and shadcn lint on each edit, colours and typography, inserting
components from the Components panel, Vue and Svelte.
