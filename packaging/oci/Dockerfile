# The image the MCP Registry's Install button runs (aegis-gys8sx):
#
#   docker run -i --rm -v quipu-data:/data ghcr.io/scbrown/quipu:<version>
#
# It packages the RELEASE binaries rather than compiling. The image is then the
# exact bytes the release smoke-tested and checksummed, and it builds in seconds
# instead of repeating a 20-minute `--features full` compile on another runner.
#
# The build context contains the one release executable, `quipu`. HTTP and
# stdio MCP enter its compiled server directly. The legacy command name is a
# relative alias for one transition release, not an independently built server.
#
# linux/amd64 only, because that is the only release target today. Docker
# Desktop on Apple Silicon runs it under emulation. Add arm64 here once the
# release matrix ships it (aegis-oifbu9).

# trixie, not bookworm: the release binaries link glibc 2.35 (ubuntu-22.04) but
# CI's smoke build links whatever ubuntu-latest has (2.39). trixie's 2.41 runs
# both, so the image CI tests and the image we ship share one base. slim already
# carries the only non-libc dependency (libstdc++6).
# Docker publishes the same official image on ECR Public. Shared CI runners
# exhausted Docker Hub's anonymous pull quota before the image smoke could run.
# Keep CI and release on this shared reference; no credentials are required.
# Pinned by digest, and this default is the digest's ONE home:
# scripts/ci/oci-base.sh reads it here, pulls it with mirror fallbacks (ECR
# Public's anonymous data limit runs out on shared runners: 429 "Data limit
# exceeded") and tags it locally. CI and the release both build FROM that tag,
# so the image CI smoke-tests and the image we ship share the same bytes
# (aegis-r38). Bump the base by changing the digest here.
ARG BASE_IMAGE=public.ecr.aws/docker/library/debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f
FROM ${BASE_IMAGE}

# The registry refuses to list the image unless this equals server.json `name`.
# scripts/ci/mcp-registry.py check keeps the two in step.
LABEL io.modelcontextprotocol.server.name="io.github.scbrown/quipu"
LABEL org.opencontainers.image.source="https://github.com/scbrown/quipu"
LABEL org.opencontainers.image.description="Quipu knowledge graph as a local MCP server over stdio"
LABEL org.opencontainers.image.licenses="Apache-2.0"

COPY --chmod=755 quipu /usr/local/bin/
RUN ln -s quipu /usr/local/bin/quipu-server

# Unprivileged, and /data owned by that user BEFORE a volume mounts there:
# Docker seeds a new named volume from the image directory, ownership included,
# so the first Install creates a volume this user can write to.
RUN useradd --system --uid 10001 --home-dir /data --no-create-home quipu \
    && mkdir /data && chown quipu:quipu /data
USER quipu
# quipu-server writes state relative to its working directory (measured: the
# verdict-signing key at .quipu/verifier.pk8). Working in /data keeps it in the
# volume; in the throwaway container layer every restart would mint a new key.
WORKDIR /data
ENV HOME=/data

# A fresh database per volume, created at first start: nothing here points at a
# server the user does not own.
ENTRYPOINT ["/usr/local/bin/quipu", "mcp", "--db", "/data/quipu.db"]
