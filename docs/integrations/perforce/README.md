# Solarxy + Helix Core (Perforce)

This recipe runs Solarxy validation as a P4 trigger so bad assets are
rejected at `p4 submit` time, before they ever land in your depot.

The integration shape: one Python script registered as a `change-submit`
trigger on your depot → script extracts pending model files via `p4 print`
→ runs `solarxy-cli analyze` against them → returns non-zero exit code on
errors → Helix Core rejects the submit and surfaces the script's stderr in
the artist's submit dialog.

---

## Prerequisites

- Helix Core 2024.2 or newer (any edition; the free Helix Core Free
  tier works for ≤5 users).
- Python 3.10+ on the P4D server (or wherever your triggers run).
- `solarxy-cli` on the trigger user's `PATH`. Install via Homebrew
  (`brew install solarxy-cli`) on macOS / Linux, or the portable `.zip`
  from [Releases][releases] on Windows servers.
- A `solarxy.toml` checked into your depot. The default path is
  `//depot/Project/solarxy.toml`; override with the
  `SOLARXY_CONFIG_DEPOT_PATH` environment variable on the trigger
  command line.

---

## Setup

### 1. Place the script

Copy `change-submit.py` from this directory to a stable location on the
P4D server. The conventional place is `/opt/solarxy/` on Linux/macOS,
`C:\Program Files\Solarxy\` on Windows:

```bash
sudo mkdir -p /opt/solarxy
sudo cp change-submit.py /opt/solarxy/change-submit.py
sudo chmod +x /opt/solarxy/change-submit.py
```

### 2. Register the trigger

As a P4 admin, edit the trigger table:

```bash
p4 triggers
```

Add an entry following this template, scoped to the depot path you want
to enforce on:

```
Triggers:
    solarxy-validate change-submit //depot/Project/... "/usr/bin/python3 /opt/solarxy/change-submit.py %changelist%"
```

The first field (`solarxy-validate`) is a label of your choosing — it
appears in `p4 triggers` output and in audit logs. The second field
(`change-submit`) is the trigger type (see below). The third (`//depot/Project/...`)
restricts the trigger to submits touching that path. The fourth is the
literal command line; `%changelist%` is substituted by Helix Core with
the pending changelist number.

#### Windows trigger line

```
Triggers:
    solarxy-validate change-submit //depot/Project/... "C:\Python310\python.exe C:\Program Files\Solarxy\change-submit.py %changelist%"
```

### 3. Verify

Open a workspace, edit an asset known to fail validation (e.g. flip a
normal in Blender and re-export), and `p4 submit`. The submit should
be rejected with output like:

```
Submit blocked by solarxy-validate:

  //depot/Project/Art/Props/barrel.glb — 2 error(s)
    FlippedNormals (mesh 0): face 47 normal opposes vertex normals
    NonManifoldEdge (edge 0): edge [v12, v15] has 3 adjacent faces
  Open locally:
    solarxy "//depot/Project/Art/Props/barrel.glb#head"

To bypass for emergencies (admin password may be required):
  p4 submit -f submitunchanged-fail
```

Fix the asset in Blender, re-export, re-submit — accepted.

---

## Trigger type: `change-submit` vs `change-content`

This script ships as a `change-submit` trigger. Helix Core fires this
**after** all changelist files have been transferred to the server but
**before** the commit becomes visible to other clients. The script can
read the pending file contents via `p4 print -o ... @=<cl>`.

The same script body also works as a `change-content` trigger (which
fires earlier in the submit pipeline) — just change the trigger-table
type field:

```
Triggers:
    solarxy-validate change-content //depot/Project/... "/usr/bin/python3 /opt/solarxy/change-submit.py %changelist%"
```

The semantic difference (per the [Helix Core triggers
documentation][p4-triggers-docs]):

| Type | Fires when | Use when |
|---|---|---|
| `change-submit` | After file transfer, before commit | You're OK with the file content existing on the server briefly even if rejected. Most studios. |
| `change-content` | After file transfer, before commit, with finer ordering guarantees | You want the trigger to gate file-transfer side effects (rare). |

For Solarxy's validation purposes, the two are functionally equivalent.
Stay with `change-submit` unless you have a specific reason to switch.

---

## Exit-code semantics

The script distinguishes three outcomes so admins can route them
differently in logs / monitoring:

| Exit | Meaning | Submit outcome |
|---|---|---|
| `0` | No model files in changelist, **or** validation passed | Accepted |
| `1` | Validation found errors | Rejected — artist sees structured findings |
| `2` | Tool error (script crashed, `solarxy-cli` not found, `p4 print` failed, etc.) | Rejected — artist sees "tool error" message; admin should investigate |

The "tool error" distinction matters: if the Solarxy infrastructure
breaks, you don't want artists thinking their assets are bad. The exit-2
message explicitly tells them to contact their admin.

---

## Rollback procedure

If the trigger misbehaves in production and is blocking legitimate
submits, an admin can disable it without removing it:

```bash
# Edit the trigger table, prefix the trigger line with `#` to disable:
p4 triggers
```

```
Triggers:
    # solarxy-validate change-submit //depot/Project/... "/usr/bin/python3 /opt/solarxy/change-submit.py %changelist%"
```

Save and exit. Submits resume working immediately — no P4D restart
required.

For a **single-changelist emergency bypass** without disabling the
trigger globally, submit with `-f`:

```bash
p4 submit -f submitunchanged-fail -c <changelist>
```

(Requires admin privileges per the standard `super` group ACL.)

---

## Troubleshooting

### `python3: command not found` in the trigger output

P4D triggers run with a minimal `PATH`. Use the absolute path to your
Python interpreter in the trigger command line (`/usr/bin/python3`,
`/usr/local/bin/python3`, or `which python3` on the trigger user's
shell).

### `solarxy-cli: command not found`

Same root cause. Either install `solarxy-cli` to a system-wide path
(`/usr/local/bin/`) or set `SOLARXY_CLI=/path/to/solarxy-cli` in the
trigger command line:

```
Triggers:
    solarxy-validate change-submit //depot/Project/... "SOLARXY_CLI=/opt/solarxy/bin/solarxy-cli /usr/bin/python3 /opt/solarxy/change-submit.py %changelist%"
```

### `p4 print` exits 1 with "no such file(s)"

The trigger user's `P4USER` / `P4PASSWD` may not have read access to
your depot. Trigger scripts typically run as the P4D service user,
which needs explicit read perms via the depot's protection table:

```
write user trigger-user * //depot/Project/...
```

(Replace `trigger-user` with whatever `P4USER` the trigger inherits;
check `p4 -Ztag info` to confirm.)

### Validation runs but findings panel is empty

The script extracts pending files via `p4 print -o ... @=<changelist>`.
For a brand-new file being added in this changelist, `@=<cl>` is the
correct revision. If you see "no model files matched", check that the
depot paths really do end in `.glb`/`.gltf`/`.obj`/`.stl`/`.ply`/`.fbx`
(the script filters on extension — case-insensitive).

### Performance on large changelists

The script copies every model file to a temp dir, then invokes
`solarxy-cli` once over the whole batch. For a 100-file changelist of
average-sized assets (~5 MB each), the round-trip is typically under 30
seconds. If your studio routinely submits hundreds of assets at once
and the trigger becomes a bottleneck, consider:

- Sharding the validation: filter `asset_files` by directory and run
  the script per shard in parallel (requires custom orchestration).
- Switching to a `change-content` trigger (same script) which has
  slightly finer queueing semantics.

---

## See also

- [`change-submit.py`](./change-submit.py) — the trigger script itself
- [GitLab CI integration](../gitlab.md) — same `solarxy-cli analyze`
  primitive, JUnit XML output, MR Tests tab integration
- [Jenkins integration](../jenkins.md) — same JUnit XML pipeline for
  Jenkins users
- [Solarxy Wiki / Installation][wiki-install] — how to install
  `solarxy-cli` on the P4D server
- [Helix Core triggers documentation][p4-triggers-docs] — full
  reference for trigger types and the trigger table format

[releases]: https://github.com/marko-koljancic/solarxy/releases
[wiki-install]: https://github.com/marko-koljancic/solarxy/wiki/Installation
[p4-triggers-docs]: https://www.perforce.com/manuals/p4sag/Content/P4SAG/scripting-triggers.html
