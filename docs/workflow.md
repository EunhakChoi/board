# How changes flow

**English** | [한국어](workflow.ko.md)

`main` is what everyone sees. Every change starts on a branch next to it, gets looked at, and joins
`main` as a new version. Nothing reaches `main` without a merge, and every version can be undone.

```
push ─▶ draft ─▶ in review ─▶ merge check ─▶ version vN on main
          │           │
          └───────────┴──▶ archived (by hand, or a draft idle for 14 days) ─▶ reopen
```

## Drafts and branches

- `charet push -m "…"` from a checkout of `main` starts a **draft** with the changes, and the
  checkout follows it. Later pushes add to it. The push prints the branch link.
- A draft is its author's: only they (or an admin) push to it, ask for review, archive or reopen
  it. Others can **duplicate** it to carry on as their own.
- `charet branch review` (or **Request review** on the branch page) opens it to the team: every
  editor may push to it, archive it or merge it, and the team is notified.
- When `main` moves, open branches **catch up** with it on their own. Files changed on both sides
  are only marked as clashing; the author settles them (`charet branch catch-up`, `charet pull`,
  fix, push).
- A draft nobody touched for 14 days is archived. Its author can reopen it.

## Looking at a branch

The branch page (**Branches** tab) shows:

- **Screens**: each screen and story before and after, side by side or one over the other. Screens
  the change can reach are marked **Changed**, new ones **New**.
- **Comments** on the branch's screens (**Comment**, then click a spot). They stay with the branch
  and do not appear on `main`. `charet comments --branch N` lists them.
- **Code**: the changed files as line diffs.
- **History**: pushes, reviews, catch-ups, clashes and approvals.

## Merging

**Merge** (or `charet branch merge`, or `charet push --merge` when working alone) needs the editor
role. Before `main` moves:

1. **Clashes** stop it: catch up and settle the files first.
2. **Protected files** need an admin's approval: package manifests and lockfiles, `vite.config.*`,
   `tsconfig*.json`, `postcss.config.*`, `tailwind.config.*`, `.npmrc` and `charet.json`. They run
   code when installed or built, so until an admin approves (**Approve protected files**,
   `charet branch approve`) the branch's previews use `main`'s versions of them. An admin merging
   approves as they go. A later push that changes them again needs a new approval.
3. **The merge check** builds what merging would give and opens the screens the change can reach.
   Problems `main` did not already have hold the merge back; the branch page lists them. While it
   builds, the web tool and the CLI wait.

The merge becomes a version (`merge`, linked to its branch). Notifications go to the branch's
author; clashes on other branches go to theirs.

## Versions and restore

**Versions** lists every version. **Restore** on one (or `charet restore vN`) makes a new version
that undoes it; history is never rewritten.

## The product repo

charet keeps its own copy of the app. Versions reach the product repo, and the repo's own changes
come back, through an admin's CLI (the web tool only shows the result):

- **Baseline**: the newest version known to match a repo commit (the last import, or the last
  version marked as handed off).
- **Hand off**: in the product repo, on a work branch, `charet handoff export -p <project> --out
  p.patch` writes what `main` changed since the baseline (without `.charet/` and `charet.json`);
  `git apply p.patch`, run the repo's checks and open a PR. Once it is merged,
  `charet handoff mark vN -p <project>` records vN and every version before it as in the repo at that
  commit. **Versions** shows the mark.
- **Drift**: `charet handoff -p <project>` (or `charet status -p <project>`) run inside the repo
  lists repo commits and uncommitted edits to the imported files since the baseline.
- **Import again**: `charet import --update -p <project>` brings the repo's files in as an `import`
  version. They are laid on the baseline and joined into `main`, so merged work not handed off yet
  stays; a file changed on both sides is refused. Open branches catch up as after any merge.

## Settings

| Setting | Default | |
|---|---|---|
| `CHARET_MERGE_CHECK` | `true` | build and check what a merge gives before `main` moves (`false` for a server without previews) |
