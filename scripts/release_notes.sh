#!/usr/bin/env bash
#
# Print the body of the single `v<version>` GitHub Release.
#
# Usage:
#   scripts/release_notes.sh <version> <owner/repo> [changelog]
#
# `changelog` defaults to `crates/server-bin/CHANGELOG.md`; the newest released
# section of that file is appended at the end.
#
# The body is written when the release is created (draft) by
# `.github/workflows/release-plz.yml`. The assets referenced below are attached
# afterwards: the shell installer / archive by cargo-dist (`release.yml`) and
# the image + chart tarballs by `build.yml`.

set -euo pipefail

version="${1:?usage: release_notes.sh <version> <owner/repo> [changelog]}"
repo="${2:?usage: release_notes.sh <version> <owner/repo> [changelog]}"
changelog="${3:-crates/server-bin/CHANGELOG.md}"

owner="${repo%%/*}"
tag="v${version}"
image="ghcr.io/${owner}/ferris-cms:${tag}"
chart="ghcr.io/${owner}/ferriscms-charts/ferriscms"

# Newest released section of the changelog (skips the [Unreleased] heading and
# stops at the next version heading).
section=""
if [ -f "$changelog" ]; then
  section=$(awk '
    /^## \[/ {
      if (started) exit
      if (index($0, "[Unreleased]")) next
      started = 1
    }
    started
  ' "$changelog")
fi

cat <<NOTES
<!-- ferriscms-install-notes -->
# ferriscms ${tag}

The CMS is a single self-contained server binary (Axum REST API + embedded
Dioxus admin UI).

## Install the server (curl)

\`\`\`sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/${repo}/releases/download/${tag}/server-bin-installer.sh | sh
\`\`\`

The same binaries are attached as
\`server-bin-x86_64-unknown-linux-gnu.tar.xz\` (plus its \`.sha256\`).

## Install with Docker

\`\`\`sh
docker pull ${image}
# or load the attached image tarball:
docker load < ferriscms-image-${version}.tgz
\`\`\`

Run it against PostgreSQL:

\`\`\`sh
docker run --rm -p 1337:1337 \\
  -e DATABASE_URL='postgres://user:pass@host:5432/ferriscms' \\
  -e JWT_SECRET='change-me-in-production' \\
  -e MEDIA_STORAGE_DIR=/data/media \\
  -v ferriscms-media:/data/media \\
  ${image}
\`\`\`

## Install with Helm

\`\`\`sh
helm install ferriscms oci://${chart} --version ${version}
# or from the attached chart tarball:
helm install ferriscms ferriscms-${version}.tgz
\`\`\`

## Assets

| Asset | What it is |
|---|---|
| \`server-bin-installer.sh\` | Shell installer for the \`ferriscms-server\` binary (the curl one-liner above) |
| \`server-bin-x86_64-unknown-linux-gnu.tar.xz\` | Prebuilt \`ferriscms-server\` binary for x86_64 Linux |
| \`ferriscms-image-${version}.tgz\` | Container image, \`docker save\`d |
| \`ferriscms-${version}.tgz\` | Packaged Helm chart |
| \`release-artifacts.txt\` | Image ref + digest and chart ref |

## Changelog

${section}
NOTES
