# Solarxy + Jenkins

This recipe runs Solarxy validation as a Jenkins pipeline stage and surfaces
findings in Jenkins's native **Test Results** panel — the same UI your team
already uses for unit-test failures.

The integration shape: one `Jenkinsfile` stage → `solarxy-cli analyze` in
the prebuilt Docker image (or installed natively) → JUnit XML →
Jenkins JUnit Plugin parses it → findings appear in the build's Tests UI.

---

## Prerequisites

- Jenkins **JUnit Plugin** installed (ships with most distributions; check
  *Manage Jenkins → Plugins*). Provides the `junit` pipeline step.
- A Linux or Windows agent. Either:
  - **Docker-capable** agent (any OS that runs the Docker plugin) → use
    the `ghcr.io/marko-koljancic/solarxy-cli` image; **or**
  - **Bare-metal** agent → install `solarxy-cli` once via the platform's
    package manager (see [Installation][wiki-install]).
- A `solarxy.toml` at the repo root (or any path the agent can read).

---

## Recipe — Linux agents

```groovy
pipeline {
  agent any
  stages {

    stage('Validate Assets') {
      steps {
        sh '''
          solarxy-cli analyze \\
            --paths "Content/**/*.glb" "Content/**/*.gltf" \\
            --config solarxy.toml \\
            --adapter generic \\
            --adapter-format junit-xml \\
            --output validation-report.xml \\
            --fail-on error
        '''
      }
      post {
        always {
          junit testResults: 'validation-report.xml',
                allowEmptyResults: true
        }
      }
    }

    stage('Build Editor') {
      // ... your existing cook / build stages
    }
  }
}
```

### Containerized variant

If your team prefers per-stage Docker images over agent-side installation:

```groovy
stage('Validate Assets') {
  agent {
    docker {
      image 'ghcr.io/marko-koljancic/solarxy-cli:0.6'
      // Mount the workspace so glob patterns resolve relative to $WORKSPACE
      args  '-v $WORKSPACE:/workspace -w /workspace'
    }
  }
  steps {
    sh '''
      solarxy-cli analyze \\
        --paths "Content/**/*.glb" \\
        --config solarxy.toml \\
        --adapter generic \\
        --adapter-format junit-xml \\
        --output validation-report.xml \\
        --fail-on error
    '''
  }
  post {
    always {
      junit testResults: 'validation-report.xml',
            allowEmptyResults: true
    }
  }
}
```

---

## Recipe — Windows agents

Unreal-pipeline studios typically run Jenkins on Windows agents. Same
recipe, swap `sh` for `bat` and use forward slashes in the glob (works
on Windows via Solarxy's path handling):

```groovy
stage('Validate Assets') {
  steps {
    bat '''
      solarxy-cli.exe analyze ^
        --paths "Content/**/*.glb" ^
        --config solarxy.toml ^
        --adapter generic ^
        --adapter-format junit-xml ^
        --output validation-report.xml ^
        --fail-on error
    '''
  }
  post {
    always {
      junit testResults: 'validation-report.xml',
            allowEmptyResults: true
    }
  }
}
```

Install `solarxy-cli.exe` on the agent via winget (`winget install
Koljam.Solarxy`) or the portable `.zip` from
[Releases][releases]. Pin the version in your agent provisioning so all
agents speak the same CLI flags.

---

## Stage placement

Run validation **before** any cook / build / package stage. Per-asset
validation takes seconds; UE cook takes minutes. A bad asset detected
upstream costs nothing; a bad asset detected mid-cook wastes an entire
build-farm slot.

```
[Checkout]
  ↓
[Validate Assets]     ← fast; fail-fast
  ↓
[Build Editor]        ← expensive
  ↓
[Cook & Package]      ← very expensive
```

The `--fail-on error` flag aborts the pipeline before cook runs if any
asset is broken.

---

## Notification hooks

Solarxy doesn't ship a Jenkins-specific notifier. Use your existing
`slackSend` / email-ext / `office365ConnectorSend` step in the
pipeline's `post` block — it picks up the validation stage outcome
automatically, since stage failures propagate.

```groovy
post {
  failure {
    slackSend channel: '#builds',
              color: 'danger',
              message: "Build #${BUILD_NUMBER} failed at stage ${env.STAGE_NAME}. See: ${BUILD_URL}"
  }
  unstable {
    // JUnit-marked unstable (test failures present but stage didn't error out):
    slackSend channel: '#builds',
              color: 'warning',
              message: "Build #${BUILD_NUMBER} has validation warnings. See: ${BUILD_URL}testReport/"
  }
}
```

---

## What appears in the Jenkins UI

| Where | What |
|---|---|
| **Test Result Trend** (job page) | Pass/fail count over recent builds. Validation failures show alongside unit-test failures. |
| **Test Results** (build page) | Per-file testcase list. Click a failed testcase → see the full issue messages from the `<failure>` body. |
| **Build status** | Red on errors (per `--fail-on`); yellow ("unstable") if a test was reported failed but the stage didn't error out — usually not the case for Solarxy since stage exit-code is governed by `--fail-on`. |

Warning-only assets stay **green** in the Test Results panel by design;
warning details surface in the testcase's `<system-out>` so reviewers
can drill in without skewing pass-rate metrics. Toggle the gating
behaviour with `--fail-on warning` if you want warnings to mark the
build red.

---

## Troubleshooting

### "no model files matched the given --paths patterns"

- The glob ran cleanly but matched nothing. Quote your `--paths`
  patterns — unquoted globs are expanded by the shell **before** the
  CLI sees them and break on nested `**`.
- On Windows agents, `bat` uses `^` for line continuation (not `\`).
  Mixing the two corrupts the command.

### JUnit Plugin reports "no test results found"

- `junit` step expects the file path relative to the workspace; the
  recipe above uses `validation-report.xml` from the current dir.
- If you ran validation in a subdirectory, supply the full path:
  `junit testResults: 'build/reports/validation.xml'`.
- `allowEmptyResults: true` (in the recipes above) suppresses the
  "0 results" warning when the validation stage was skipped due to
  upstream failure.

### Stage marked "unstable" instead of "failed"

The JUnit Plugin's default is to mark the build **unstable** (yellow)
when failed tests are present, regardless of the stage's exit code.
To force a failed build:

```groovy
junit testResults: 'validation-report.xml',
      allowEmptyResults: true,
      keepLongStdio: true,
      skipPublishingChecks: true
// Then:
script {
  if (currentBuild.result == 'UNSTABLE') {
    error 'Validation produced failures; failing the build.'
  }
}
```

Or simpler: rely solely on `--fail-on error`'s non-zero exit and skip
the `junit` step's unstable-by-default behaviour by checking
`solarxy-cli`'s exit code yourself.

---

## See also

- [`Dockerfile.cli`](../../Dockerfile.cli) — produces the GHCR image
  used in the containerized variant
- [GitLab CI integration](./gitlab.md) — same JUnit XML pipeline for
  GitLab users
- [Solarxy Wiki / Installation][wiki-install] — agent-side install
  options
- [Solarxy Wiki / Configuration][wiki-config] — `solarxy.toml`
  reference

[wiki-install]: https://github.com/marko-koljancic/solarxy/wiki/Installation
[wiki-config]: https://github.com/marko-koljancic/solarxy/wiki/Configuration
[releases]: https://github.com/marko-koljancic/solarxy/releases
