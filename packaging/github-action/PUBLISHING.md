# Publishing the action to its own repository

This directory holds the canonical, versioned source for the
`solarxy-validate-action`. GitHub Actions Marketplace requires actions
to live at a repo root, so the action is **mirrored** to a separate
public repository at publish time:

- **Canonical source (this repo):** `packaging/github-action/`
- **Published mirror:** `github.com/marko-koljancic/solarxy-validate-action`

Keeping the source here means changes to the action are reviewed
alongside the CLI it wraps, and a single PR can update both
(e.g. adding a new `--adapter-format` flag + the action's input).

---

## First-time setup of the external repo

1. Create an empty public repo `marko-koljancic/solarxy-validate-action`
   on GitHub. No template, no README — we'll seed it from here.

2. From this directory, mirror the contents:

   ```bash
   cd packaging/github-action
   git init /tmp/solarxy-validate-action
   cp action.yml README.md /tmp/solarxy-validate-action/
   cp ../../LICENSE /tmp/solarxy-validate-action/    # MIT, same as solarxy
   cd /tmp/solarxy-validate-action
   git add -A
   git commit -m "Initial commit — mirror of solarxy/packaging/github-action @ <commit-sha>"
   git remote add origin git@github.com:marko-koljancic/solarxy-validate-action.git
   git branch -M main
   git push -u origin main
   ```

3. Tag the first release:

   ```bash
   git tag v1.0.0
   git tag v1
   git push --tags
   ```

   Maintain `v1` as a mutable major tag pointing at the latest `v1.x.y`
   so consumers using `@v1` automatically pick up bug fixes. Cut new
   tags (`v1.1.0` etc.) on every change; move `v1` to match.

4. **Marketplace listing:** open the new repo's *Releases* page → create
   release from tag `v1.0.0` → tick "Publish this Action to the GitHub
   Marketplace" → select category `Code quality` → submit. Marketplace
   review is usually instant for new actions from established accounts.

---

## Per-release sync workflow (after first-time setup)

When `action.yml` or `README.md` changes here:

1. Review and merge the change in this repo.
2. Run the sync workflow (manual today; can be automated later):

   ```bash
   cd packaging/github-action
   # Replace the mirror contents wholesale to avoid drift bugs.
   rsync -a --delete \
     --include='action.yml' --include='README.md' --include='LICENSE' \
     --exclude='*' \
     ./ /path/to/local/checkout/of/solarxy-validate-action/

   cd /path/to/local/checkout/of/solarxy-validate-action
   git add -A
   git commit -m "Sync from solarxy@<commit-sha>"
   ```

3. Tag and push:

   ```bash
   # New patch / minor — for bug fixes / additive inputs:
   git tag v1.0.1                 # patch
   git tag -f v1                  # force-move the floating major tag
   git push --tags --force-with-lease
   ```

   Use `--force-with-lease` (never `--force`) when moving `v1` so a
   concurrent push from someone else isn't blindly overwritten.

4. Update the consumer-side `@v1.x` pin in this repo's
   `docs/integrations/` and any internal CI workflows.

---

## Why not a Git subtree / submodule?

Considered and rejected:

- **Submodule:** consumers of the action repo would need to recursively
  clone — but Marketplace actions are downloaded as a single tarball,
  so submodule references aren't resolved at runtime. Action would be
  broken.
- **Subtree:** doable, but requires `git subtree push/pull` discipline
  on every change, and the mirror commits look noisy (every history
  rewrite). The simple rsync-then-commit pattern above is easier to
  audit and reverse.

The duplication cost is bounded: `action.yml` is ~270 LOC, `README.md`
is ~200 LOC. A pre-publish CI check (future task) can `diff` the two
locations and warn on drift.

---

## Future: automated sync

Once the action is live, a follow-up task can add
`.github/workflows/sync-action.yml` that:

1. Triggers on push to `main` when `packaging/github-action/**` changes.
2. Uses a deploy key or PAT for the external repo.
3. Diffs and pushes the canonical copy; opens a release PR when
   `action.yml` changes.

Deferring this until the action has been live for a few releases — manual
sync is fine while behaviour is still settling.
