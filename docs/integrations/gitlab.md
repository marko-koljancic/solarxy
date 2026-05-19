# Solarxy + GitLab CI

This recipe wires Solarxy's per-commit asset validation into GitLab CI so
findings surface in the merge-request **Tests** tab — the same UI your team
already watches for code-side test results.

The integration shape: one `.gitlab-ci.yml` job → `solarxy-cli analyze`
inside our prebuilt Docker image → JUnit XML artifact → GitLab's native
`artifacts:reports:junit` picks it up.

---

## Prerequisites

- A `solarxy.toml` at the repo root (or anywhere the GitLab runner can read
  it). See [Solarxy Wiki / Configuration][wiki-config] for the field
  reference, or scaffold one with `solarxy-cli config init` (coming in
  0.6.1).
- A GitLab project with CI enabled. Works on both GitLab.com and
  self-hosted GitLab. The recipe assumes a Linux runner; Windows runners
  work too — substitute `solarxy-cli.exe` and a Windows-compatible image.

---

## The recipe

Add this job to your `.gitlab-ci.yml`:

```yaml
validate-assets:
  image: ghcr.io/marko-koljancic/solarxy-cli:0.6
  script:
    - solarxy-cli analyze
        --paths "assets/**/*.glb" "assets/**/*.gltf"
        --config solarxy.toml
        --adapter generic
        --adapter-format junit-xml
        --output validation-report.xml
        --fail-on error
  artifacts:
    when: always
    reports:
      junit: validation-report.xml
    paths:
      - validation-report.xml
    expire_in: 30 days
  rules:
    - changes:
        - "assets/**/*"
        - "solarxy.toml"
```

That's it. No GitLab-specific plugins, no shell glue.

### Tagging convention

The Docker image follows semver-major tags:

| Tag | Meaning | When to use |
|---|---|---|
| `ghcr.io/marko-koljancic/solarxy-cli:latest` | Most recent stable release | Trying things out; not recommended for pinned CI |
| `ghcr.io/marko-koljancic/solarxy-cli:0.6` | Latest patch of the 0.6.x line | **Recommended** — picks up bug fixes, no breaking changes |
| `ghcr.io/marko-koljancic/solarxy-cli:0.6.0` | Exact version | Reproducible builds; pin during release windows |

---

## What it produces

### In the MR "Tests" tab

Every `validate-assets` run with errors surfaces failed test entries —
one per problematic asset, with the failing-check names + messages
nested. GitLab renders them alongside your code-side test failures
(rspec, jest, cargo-test, etc.) with the same red/green chrome.

Warning-only assets stay **green** in the test panel by design; their
warning details surface in the testcase's `<system-out>` so reviewers
can drill in without the build being marked red. The `--fail-on`
flag governs the overall pipeline pass/fail:

| `--fail-on` value | MR passes when |
|---|---|
| `error` (default) | No assets produce errors |
| `warning` | No assets produce errors **or** warnings |
| `never` | Always passes; report is purely informational |

### The artifact

`validation-report.xml` is uploaded as a build artifact and retained
for 30 days. Useful for:

- Auditing historical asset-quality trends
- Re-rendering the report locally with a different format
  (`solarxy-cli` reads the JSON form, not XML — for historical
  re-analysis run with `--adapter-format json` instead)

---

## Variants

### Pre-merge enforcement

The recipe above runs on every commit. To restrict the failing gate to
merge requests only (and keep branch builds informational), split the
job:

```yaml
validate-assets:
  image: ghcr.io/marko-koljancic/solarxy-cli:0.6
  script:
    - solarxy-cli analyze
        --paths "assets/**/*.glb"
        --config solarxy.toml
        --adapter generic
        --adapter-format junit-xml
        --output validation-report.xml
        --fail-on never        # informational on branch builds
  artifacts:
    when: always
    reports:
      junit: validation-report.xml
  rules:
    - if: $CI_PIPELINE_SOURCE != "merge_request_event"

validate-assets-mr:
  extends: validate-assets
  script:
    - solarxy-cli analyze
        --paths "assets/**/*.glb"
        --config solarxy.toml
        --adapter generic
        --adapter-format junit-xml
        --output validation-report.xml
        --fail-on error        # gate the merge
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
```

### Self-hosted runners without internet

If your runners can't reach ghcr.io, pull and re-publish the image to
your internal registry:

```bash
docker pull ghcr.io/marko-koljancic/solarxy-cli:0.6
docker tag ghcr.io/marko-koljancic/solarxy-cli:0.6 \
           registry.internal/solarxy-cli:0.6
docker push registry.internal/solarxy-cli:0.6
```

Then point `image:` at `registry.internal/solarxy-cli:0.6`.

### Mixed-content repos (assets + code)

Use `rules:changes:` to only run validation when assets change. The
top-level recipe shows this; for code-only commits the job is skipped
entirely so your branch builds don't pay the validation cost.

---

## Troubleshooting

### "no model files matched the given --paths patterns"

The glob ran cleanly but matched nothing. Check that:
- `--paths` is in **quotes** — unquoted globs are expanded by the shell
  before they reach the CLI, which breaks on nested `**`.
- The patterns are relative to GitLab's checkout directory
  (`$CI_PROJECT_DIR` aka `/builds/<group>/<project>`).

### "no model files matched" but the assets exist

`rules:changes:` may have skipped the job (only matches paths in the
changeset). Check the job log for the "skipped: rules" message.

### Findings missing from the MR Tests tab

GitLab requires the JUnit XML to be uploaded under
`artifacts:reports:junit`. The recipe above does this; if you adapt it,
make sure both `artifacts:reports:junit:` **and** the literal
`validation-report.xml` path under `artifacts:paths:` are present —
without the latter, the XML disappears after the job and isn't
browsable for historical review.

### `--fail-on warning` is too strict / too lax for our team

`--fail-on warning` blocks the merge on either errors **or** warnings.
`--fail-on error` (default) lets warnings through. There's no
per-rule severity override at the CLI level today — tune the
`solarxy.toml` `[validation]` table to flip rules between `"error"`,
`"warning"`, and `"off"` if you want different gating behaviour.

---

## See also

- [`Dockerfile.cli`](../../Dockerfile.cli) — the Dockerfile that
  produces the published image
- [`packaging/winget/`](../../packaging/winget/) and
  [`packaging/homebrew/`](../../packaging/homebrew/) — non-CI install
  paths for local dev workstations
- [Jenkins integration](./jenkins.md) — same JUnit XML pipeline for
  Jenkins users
- [Solarxy Wiki / Configuration][wiki-config] — full `solarxy.toml`
  reference

[wiki-config]: https://github.com/marko-koljancic/solarxy/wiki/Configuration
