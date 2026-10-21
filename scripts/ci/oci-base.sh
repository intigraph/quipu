#!/usr/bin/env bash
# Pull the MCP Registry image's base by digest and tag it locally.
#
#   scripts/ci/oci-base.sh <local-tag>
#   docker build --build-arg BASE_IMAGE=<local-tag> -f packaging/oci/Dockerfile ...
#
# The digest has ONE home: the BASE_IMAGE default in packaging/oci/Dockerfile.
# ci.yml and mcp-registry.yml both call this script, so the image CI smoke-tests
# and the image a release ships are built FROM the same bytes (aegis-r38). Before
# this, CI pinned a digest inline and the release built FROM the floating
# trixie-slim tag.
#
# Both registries serve the same content-addressed index, so the pull tries each
# in turn: anonymous ECR Public pulls hit "429 Data limit exceeded" on shared
# runners. The build then uses only the local tag.
set -euo pipefail

tag=${1:?usage: oci-base.sh <local-tag>}
dockerfile=${OCI_DOCKERFILE:-packaging/oci/Dockerfile}

ref=$(sed -n 's/^ARG BASE_IMAGE=//p' "$dockerfile")
digest=${ref##*@}
# Refuse an unpinned default rather than pulling whatever the tag points at today.
if [[ "$ref" != *@sha256:* || ! "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "::error::$dockerfile BASE_IMAGE default is not pinned by digest: '${ref}'" >&2
  exit 1
fi

pulled=
for attempt in 1 2 3; do
  for repo in mirror.gcr.io/library/debian public.ecr.aws/docker/library/debian; do
    if docker pull --quiet "$repo@$digest"; then
      pulled="$repo@$digest"
      break 2
    fi
  done
  sleep $((attempt * 20))
done
if [ -z "$pulled" ]; then
  echo "::error::base image $digest unavailable from every mirror" >&2
  exit 1
fi
docker tag "$pulled" "$tag"
echo "base $digest from ${pulled%@*} tagged $tag"
