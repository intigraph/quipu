# Agent Setup

Install [Quipu and its companion server](installation.md) before registering a
client. [Quick Start](quick-start.md) covers direct CLI queries;
[The AI Agent Builder](../tutorials/agent-builder.md) covers a multi-agent workflow.

## Paste-in setup prompt

Give a coding agent this prompt, with your client and store details filled in:

```text
Set up Quipu for this project in my coding-agent client: <client>.
Use this executable directory: <absolute directory>, and this database path
or HTTP server: <absolute database path or server URL>.

Start at https://github.com/intigraph/quipu/blob/main/docs/book/src/getting-started/agent-setup.md
and follow its primer, skill and repository-pendant links. Inspect existing
client configuration and preserve other servers and databases. Explain the planned
changes before applying them. Use the documented local stdio or HTTP MCP
connection; keep credentials in private files or the client's secret mechanism,
never in this conversation. Verify tool discovery and require
SELECT (1 AS ?connection_control) WHERE {} to return connection_control = 1.
Do not test writes in a shared store. Read the Quipu skill for query-first
work. For repository orientation, verify the release pendant and review it
in a separate local store's quarantine; do not promote it into ROOT without
my approval. Report the configuration changed, versions used, verification
results and any unknowns.
```

Use these agent-facing surfaces alongside the prompt:

- **MCP connection:** [local stdio](#local-stdio-mcp) or
  [HTTP](#connect-to-an-http-server), with the [MCP tool reference](../reference/mcp-tools.md).
- **Instructions:** the [Agent Primer](agent-primer.md) and the checked-in
  [Quipu skill](https://github.com/intigraph/quipu/blob/main/skills/quipu/SKILL.md).
  The optional [embedded-artifacts skill](https://github.com/intigraph/quipu/blob/main/skills/quipu-embedded-artifacts/SKILL.md)
  covers graph-backed artifact views; it is not needed for an MCP connection.
- **Repository knowledge:** the [release repository pendant](https://github.com/intigraph/quipu/releases/latest),
  its checksum and provenance, and the [verify, quarantine, promote workflow](../sharing/index.md#receiving-verify-quarantine-promote).
  A pendant supplies source-linked knowledge; it does not configure the client.

## Local stdio MCP

Keep `quipu` and `quipu-server` from the same release in the same directory.
Use absolute executable and database paths so a client started in another
working directory still opens the intended store:

```sh
claude mcp add quipu -- /absolute/path/to/quipu mcp --db /absolute/path/to/store.db
```

`quipu mcp` launches the sibling server over stdio; it does not expose an HTTP
listener. Stop the client to stop its child server. Diagnostics belong on
stderr; stdout is reserved for MCP messages.

For a store configured to require write authentication, append
`--mcp-token-file /absolute/path/to/private-token`. The token file must be a
regular private file (0600 or 0400 on Unix), no more than 4096 bytes. Keep its
contents out of prompts, command arguments and version control. The flag
selects an existing credential; it does not issue one.

## Connect to an HTTP server

Register the running server's `/mcp` endpoint as streamable HTTP in your
client. Configure the server's write bearer through its private configuration
and the client's supported secret mechanism. Read-only mode continues to refuse
writes even with a credential. See [Configuration](configuration.md) and the
[MCP connection contract](../reference/mcp-tools.md#connect-directly).

For a local standalone server:

```sh
quipu-server --db /absolute/path/to/store.db --bind 127.0.0.1:3030
curl --fail http://127.0.0.1:3030/health
```

A healthy HTTP endpoint proves the server answers, not that a particular client
has connected or can write.

## Verify the client's connection

Ask the client to list the server's tools, then call `quipu_query` with:

```sparql
SELECT (1 AS ?connection_control) WHERE {}
```

Require the result `connection_control = 1`. An empty result or an error is not
a successful connection. A query proves read access only. To verify writes,
use a disposable store and types from that store's loaded vocabulary; read
back the exact new subject. Do not create a test entity in a shared store.
Natural-language search additionally requires an embedding provider; it is not
a suitable first connection check.

## Registry and container clients

The repository's `server.json` names `io.github.intigraph/quipu` in the MCP
Registry and pins an OCI image. A registry-aware client may install that
entry; inspect its pinned version rather than assuming it matches the latest
CLI release. The image requires a compatible container runtime.

A named volume retains `/data/quipu.db` across container removal. See
[container installation and removal](installation.md#container-and-mcp-registry)
before changing or deleting it.

## Disconnect and remove client configuration

For the Claude CLI registration above:

```sh
claude mcp remove quipu
```

For other clients, remove the server entry through that client's configuration
surface and stop its child process or container. Remove any dedicated token
file only after disconnecting all clients that use it. Removing registration
does not remove the executable or erase its database; follow
[Installation](installation.md#remove-configuration-and-data) for those steps.
