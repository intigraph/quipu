# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_qp_global_optspecs
    string join \n db= h/help
end

function __fish_qp_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_qp_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_qp_using_subcommand
    set -l cmd (__fish_qp_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c qp -n "__fish_qp_needs_command" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c qp -n "__fish_qp_needs_command" -f -a "store" -d 'Inspect native ownership, or explicitly adopt or restore a disk store'
complete -c qp -n "__fish_qp_needs_command" -f -a "version" -d 'Print the version and source commit'
complete -c qp -n "__fish_qp_needs_command" -f -a "install-alias" -d 'Add the qp alias beside the quipu binary'
complete -c qp -n "__fish_qp_needs_command" -f -a "knot" -d 'Assert Turtle facts, validated against loaded shapes'
complete -c qp -n "__fish_qp_needs_command" -f -a "load" -d 'Assert Turtle facts (alias of knot), or load a project bundle directory'
complete -c qp -n "__fish_qp_needs_command" -f -a "ingest" -d 'Bulk-load an N-Triples, Turtle or N-Quads file into one graph with declared count and digest'
complete -c qp -n "__fish_qp_needs_command" -f -a "read" -d 'Run a SPARQL query, optionally as of a valid time or transaction'
complete -c qp -n "__fish_qp_needs_command" -f -a "query" -d 'Run a SPARQL query (alias of read)'
complete -c qp -n "__fish_qp_needs_command" -f -a "rule-query" -d 'Read typed ROOT support and the admitted judgment for a rule request'
complete -c qp -n "__fish_qp_needs_command" -f -a "cord" -d 'List entities, optionally of one type'
complete -c qp -n "__fish_qp_needs_command" -f -a "unravel" -d 'Show facts as they were at a transaction or valid time'
complete -c qp -n "__fish_qp_needs_command" -f -a "impact" -d 'Walk what depends on an entity, optionally after a speculative removal'
complete -c qp -n "__fish_qp_needs_command" -f -a "explain" -d 'Trace a derived fact back to its premises'
complete -c qp -n "__fish_qp_needs_command" -f -a "project" -d 'Project the graph and run a graph algorithm such as PageRank'
complete -c qp -n "__fish_qp_needs_command" -f -a "report" -d 'Report hubs, surprising links and open questions in the graph'
complete -c qp -n "__fish_qp_needs_command" -f -a "reason" -d 'Run Datalog rules to a fixpoint and persist the derivations'
complete -c qp -n "__fish_qp_needs_command" -f -a "demotions" -d 'Read preserved unsupported demotions without materializing a closure'
complete -c qp -n "__fish_qp_needs_command" -f -a "episode" -d 'Ingest a structured episode of nodes and edges from JSON'
complete -c qp -n "__fish_qp_needs_command" -f -a "retract" -d 'Retract an entity\'s facts, optionally one predicate; history is kept'
complete -c qp -n "__fish_qp_needs_command" -f -a "shapes" -d 'Load, list or remove the SHACL shapes that guard writes'
complete -c qp -n "__fish_qp_needs_command" -f -a "namespaces" -d 'List or apply the owner namespace registry'
complete -c qp -n "__fish_qp_needs_command" -f -a "entity-ref" -d 'Mint or resolve stable entity references'
complete -c qp -n "__fish_qp_needs_command" -f -a "ontology" -d 'Load, list or remove OWL ontologies (owl feature)'
complete -c qp -n "__fish_qp_needs_command" -f -a "validate" -d 'Validate a data file against a shapes file without writing'
complete -c qp -n "__fish_qp_needs_command" -f -a "repl" -d 'Open an interactive SPARQL prompt'
complete -c qp -n "__fish_qp_needs_command" -f -a "export" -d 'Export RDF from ROOT, one graph, a provenance group or a CONSTRUCT query'
complete -c qp -n "__fish_qp_needs_command" -f -a "render" -d 'Render graph data into files in an owned directory'
complete -c qp -n "__fish_qp_needs_command" -f -a "stats" -d 'Show store statistics'
complete -c qp -n "__fish_qp_needs_command" -f -a "prime" -d 'Print the agent primer and a bounded, read-only summary of the store'
complete -c qp -n "__fish_qp_needs_command" -f -a "search" -d 'Search entities by keyword, vector or hybrid ranking'
complete -c qp -n "__fish_qp_needs_command" -f -a "search-index" -d 'Report, backfill or drop the search indexes'
complete -c qp -n "__fish_qp_needs_command" -f -a "doctor" -d 'Report graph-label drift and recommended floors'
complete -c qp -n "__fish_qp_needs_command" -f -a "pack" -d 'Export a graph, or the whole store, as a pack; or verify a pack'
complete -c qp -n "__fish_qp_needs_command" -f -a "unpack" -d 'Merge a published pack into a local graph'
complete -c qp -n "__fish_qp_needs_command" -f -a "restore" -d 'Replace the store with a full pack, binary or text'
complete -c qp -n "__fish_qp_needs_command" -f -a "share" -d 'Write a portable share of a graph, scope or project'
complete -c qp -n "__fish_qp_needs_command" -f -a "status" -d 'Report how a share has diverged from this store'
complete -c qp -n "__fish_qp_needs_command" -f -a "merge" -d 'Merge a share back into ROOT, with reviewable conflict decisions'
complete -c qp -n "__fish_qp_needs_command" -f -a "import" -d 'Verify a share into quarantine, then promote or review it'
complete -c qp -n "__fish_qp_needs_command" -f -a "compose" -d 'Compose verified packs into a named inspection dataset'
complete -c qp -n "__fish_qp_needs_command" -f -a "changes" -d 'Read whole transaction change pages from the change feed'
complete -c qp -n "__fish_qp_needs_command" -f -a "follow" -d 'Keep this database a read-only follower of a primary server'
complete -c qp -n "__fish_qp_needs_command" -f -a "acquire" -d 'Measure the bounded scope of a direct-main graph acquisition'
complete -c qp -n "__fish_qp_needs_command" -f -a "migrate-vectors" -d 'Move stored vectors between backends (lancedb feature)'
complete -c qp -n "__fish_qp_needs_command" -f -a "git-merge" -d 'Merge pendants from Git snapshots and stop before committing'
complete -c qp -n "__fish_qp_needs_command" -f -a "merge-driver" -d 'Low-level Git merge driver for pendant files'
complete -c qp -n "__fish_qp_needs_command" -f -a "pendant-resolve" -d 'Record a resolution choice for one pendant merge conflict'
complete -c qp -n "__fish_qp_needs_command" -f -a "pendant-check" -d 'Check a pendant merge result in CI without a merge driver'
complete -c qp -n "__fish_qp_needs_command" -f -a "diff-textconv" -d 'Render a pack with labels for git diff textconv'
complete -c qp -n "__fish_qp_needs_command" -f -a "mcp" -d 'Serve the MCP tools over stdio through the sibling quipu-server'
complete -c qp -n "__fish_qp_needs_command" -f -a "hook" -d 'Run a harness hook such as session capture'
complete -c qp -n "__fish_qp_needs_command" -f -a "hooks" -d 'Bundle, install, uninstall or check quipu\'s Claude Code and Codex hooks'
complete -c qp -n "__fish_qp_needs_command" -f -a "policy" -d 'Draft an advisory policy from an exemplar, or backtest one'
complete -c qp -n "__fish_qp_needs_command" -f -a "gate" -d 'Judge a candidate policy set over recorded history without writing'
complete -c qp -n "__fish_qp_needs_command" -f -a "path" -d 'Golden-path analysis: provenance cone, backtest and draft'
complete -c qp -n "__fish_qp_needs_command" -f -a "align" -d 'Propose, decide and apply cross-graph alignments'
complete -c qp -n "__fish_qp_needs_command" -f -a "propose" -d 'List, submit, plan, accept or reject schema change proposals'
complete -c qp -n "__fish_qp_needs_command" -f -a "memory" -d 'Read, export, receipt or commit versioned file memory'
complete -c qp -n "__fish_qp_needs_command" -f -a "attest" -d 'Register and manage attested session bindings'
complete -c qp -n "__fish_qp_needs_command" -f -a "signer" -d 'Enroll and manage hardware signers for attested writes'
complete -c qp -n "__fish_qp_needs_command" -f -a "deck" -d 'Configure and enroll the Decision Deck'
complete -c qp -n "__fish_qp_needs_command" -f -a "trust-root" -d 'Enroll the first human key of the root of trust'
complete -c qp -n "__fish_qp_needs_command" -f -a "graph" -d 'Create, label, list, import, freeze and thaw named graphs and overlays'
complete -c qp -n "__fish_qp_needs_command" -f -a "db" -d 'Respace, attach or migrate the store file'
complete -c qp -n "__fish_qp_needs_command" -f -a "events" -d 'Read store event records such as refusals'
complete -c qp -n "__fish_qp_needs_command" -f -a "fork" -d 'Create, list, diff, drop or promote named forks of ROOT'
complete -c qp -n "__fish_qp_needs_command" -f -a "audit" -d 'Check traces, namespaces, replays and quarantine against policy'
complete -c qp -n "__fish_qp_using_subcommand store; and not __fish_seen_subcommand_from status adopt restore" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand store; and not __fish_seen_subcommand_from status adopt restore" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand store; and not __fish_seen_subcommand_from status adopt restore" -f -a "status" -d 'Report inactive or enforced native store ownership without adopting'
complete -c qp -n "__fish_qp_using_subcommand store; and not __fish_seen_subcommand_from status adopt restore" -f -a "adopt" -d 'Bind an existing native store to this host and path with a fresh epoch'
complete -c qp -n "__fish_qp_using_subcommand store; and not __fish_seen_subcommand_from status adopt restore" -f -a "restore" -d 'Restore a SQLite snapshot to a new path and explicitly adopt it'
complete -c qp -n "__fish_qp_using_subcommand store; and __fish_seen_subcommand_from status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand store; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand store; and __fish_seen_subcommand_from adopt" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand store; and __fish_seen_subcommand_from adopt" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand store; and __fish_seen_subcommand_from restore" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand store; and __fish_seen_subcommand_from restore" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand version" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand version" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand install-alias" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand install-alias" -l bin-dir -d 'Directory containing the CLI alias' -r
complete -c qp -n "__fish_qp_using_subcommand install-alias" -l check -d 'Check the alias without installing it'
complete -c qp -n "__fish_qp_using_subcommand install-alias" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand knot" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand knot" -l shapes -d 'Shapes used for validation or packaging' -r
complete -c qp -n "__fish_qp_using_subcommand knot" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand knot" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand knot" -l blank-node-scope -d 'Blank-node identity scope for this import' -r
complete -c qp -n "__fish_qp_using_subcommand knot" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand load" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand load" -l shapes -d 'Shapes used for validation or packaging' -r
complete -c qp -n "__fish_qp_using_subcommand load" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand load" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand load" -l blank-node-scope -d 'Blank-node identity scope for this import' -r
complete -c qp -n "__fish_qp_using_subcommand load" -l destination -d 'Publication destination evaluated by policy' -r
complete -c qp -n "__fish_qp_using_subcommand load" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand load" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand ingest" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l declare-count -d 'Expected ingest triple count' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l declare-sha256 -d 'Expected input SHA-256 digest' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l format -d 'Output or input format for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l chunk -d 'Maximum triples per ingest chunk' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l base-iri -d 'Base IRI for relative RDF references' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l source -d 'Source identifier recorded in provenance' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l blank-node-scope -d 'Blank-node identity scope for this import' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand read" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand read" -l valid-at -d 'Valid-time instant to query' -r
complete -c qp -n "__fish_qp_using_subcommand read" -l tx -d 'Historical transaction to read' -r
complete -c qp -n "__fish_qp_using_subcommand read" -l fork -d 'Named store fork to query' -r
complete -c qp -n "__fish_qp_using_subcommand read" -l entailment -d 'Query entailment regime' -r
complete -c qp -n "__fish_qp_using_subcommand read" -l federation -d 'Enable configured remote query federation'
complete -c qp -n "__fish_qp_using_subcommand read" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand query" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand query" -l valid-at -d 'Valid-time instant to query' -r
complete -c qp -n "__fish_qp_using_subcommand query" -l tx -d 'Historical transaction to read' -r
complete -c qp -n "__fish_qp_using_subcommand query" -l fork -d 'Named store fork to query' -r
complete -c qp -n "__fish_qp_using_subcommand query" -l entailment -d 'Query entailment regime' -r
complete -c qp -n "__fish_qp_using_subcommand query" -l federation -d 'Enable configured remote query federation'
complete -c qp -n "__fish_qp_using_subcommand query" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand rule-query" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand rule-query" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand cord" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand cord" -l type -d 'Entity type IRI to select' -r
complete -c qp -n "__fish_qp_using_subcommand cord" -l limit -d 'Maximum number of results' -r
complete -c qp -n "__fish_qp_using_subcommand cord" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand unravel" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand unravel" -l tx -d 'Historical transaction to read' -r
complete -c qp -n "__fish_qp_using_subcommand unravel" -l valid-at -d 'Valid-time instant to query' -r
complete -c qp -n "__fish_qp_using_subcommand unravel" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand impact" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand impact" -l hops -d 'Maximum traversal hops' -r
complete -c qp -n "__fish_qp_using_subcommand impact" -l predicate -d 'Predicate IRI used to filter facts or edges' -r
complete -c qp -n "__fish_qp_using_subcommand impact" -l remove -d 'Calculate the impact of removing the entity'
complete -c qp -n "__fish_qp_using_subcommand impact" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand explain" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand explain" -l depth -d 'Maximum explanation depth' -r
complete -c qp -n "__fish_qp_using_subcommand explain" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand project" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l algorithm -d 'Graph projection algorithm' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l type -d 'Entity type IRI to select' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l predicate -d 'Predicate IRI used to filter facts or edges' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l limit -d 'Maximum number of results' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l damping -d 'PageRank damping factor' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l max-iters -d 'Maximum algorithm iterations' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l tolerance -d 'Convergence tolerance for the algorithm' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l from -d 'Start entity IRI for shortest-path projection' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l to -d 'Destination entity IRI for shortest-path projection' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l seed -d 'Seed entity for graph projection' -r
complete -c qp -n "__fish_qp_using_subcommand project" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand report" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand report" -l type -d 'Entity type IRI to select' -r
complete -c qp -n "__fish_qp_using_subcommand report" -l predicate -d 'Predicate IRI used to filter facts or edges' -r
complete -c qp -n "__fish_qp_using_subcommand report" -l hubs -d 'Number of hubs in the report' -r
complete -c qp -n "__fish_qp_using_subcommand report" -l surprises -d 'Number of surprising relationships in the report' -r
complete -c qp -n "__fish_qp_using_subcommand report" -l questions -d 'Number of suggested questions in the report' -r
complete -c qp -n "__fish_qp_using_subcommand report" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand reason" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand reason" -l rules -d 'Rule set to load or evaluate' -r
complete -c qp -n "__fish_qp_using_subcommand reason" -l reactive -d 'Use the reactive reasoner'
complete -c qp -n "__fish_qp_using_subcommand reason" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand demotions" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand demotions" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand demotions" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand episode" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand episode" -l base-ns -d 'Base namespace for episode entities' -r
complete -c qp -n "__fish_qp_using_subcommand episode" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand episode" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand retract" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand retract" -l predicate -d 'Predicate IRI used to filter facts or edges' -r
complete -c qp -n "__fish_qp_using_subcommand retract" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand retract" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand shapes; and not __fish_seen_subcommand_from list load remove" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand shapes; and not __fish_seen_subcommand_from list load remove" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand shapes; and not __fish_seen_subcommand_from list load remove" -f -a "list" -d 'List the loaded SHACL shapes'
complete -c qp -n "__fish_qp_using_subcommand shapes; and not __fish_seen_subcommand_from list load remove" -f -a "load" -d 'Load a named SHACL shapes file; later writes are validated against it'
complete -c qp -n "__fish_qp_using_subcommand shapes; and not __fish_seen_subcommand_from list load remove" -f -a "remove" -d 'Remove a named shapes file'
complete -c qp -n "__fish_qp_using_subcommand shapes; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand shapes; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand shapes; and __fish_seen_subcommand_from load" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand shapes; and __fish_seen_subcommand_from load" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand shapes; and __fish_seen_subcommand_from remove" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand shapes; and __fish_seen_subcommand_from remove" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand namespaces; and not __fish_seen_subcommand_from list apply" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand namespaces; and not __fish_seen_subcommand_from list apply" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand namespaces; and not __fish_seen_subcommand_from list apply" -f -a "list" -d 'List the effective owner namespace registry'
complete -c qp -n "__fish_qp_using_subcommand namespaces; and not __fish_seen_subcommand_from list apply" -f -a "apply" -d 'Apply the configured owner namespace registry to the store'
complete -c qp -n "__fish_qp_using_subcommand namespaces; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand namespaces; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand namespaces; and __fish_seen_subcommand_from apply" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand namespaces; and __fish_seen_subcommand_from apply" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and not __fish_seen_subcommand_from mint resolve verify" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and not __fish_seen_subcommand_from mint resolve verify" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and not __fish_seen_subcommand_from mint resolve verify" -f -a "mint" -d 'Mint a typed stable entity reference from a JSON request'
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and not __fish_seen_subcommand_from mint resolve verify" -f -a "resolve" -d 'Resolve a stable entity reference from a JSON request'
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and not __fish_seen_subcommand_from mint resolve verify" -f -a "verify" -d 'Verify a current key IRI against its local key triples'
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and __fish_seen_subcommand_from mint" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and __fish_seen_subcommand_from mint" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and __fish_seen_subcommand_from resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and __fish_seen_subcommand_from resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and __fish_seen_subcommand_from verify" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand entity-ref; and __fish_seen_subcommand_from verify" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand ontology; and not __fish_seen_subcommand_from list load remove" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand ontology; and not __fish_seen_subcommand_from list load remove" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand ontology; and not __fish_seen_subcommand_from list load remove" -f -a "list" -d 'List loaded OWL ontologies'
complete -c qp -n "__fish_qp_using_subcommand ontology; and not __fish_seen_subcommand_from list load remove" -f -a "load" -d 'Load an OWL ontology and materialize its entailments'
complete -c qp -n "__fish_qp_using_subcommand ontology; and not __fish_seen_subcommand_from list load remove" -f -a "remove" -d 'Remove a loaded OWL ontology'
complete -c qp -n "__fish_qp_using_subcommand ontology; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand ontology; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand ontology; and __fish_seen_subcommand_from load" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand ontology; and __fish_seen_subcommand_from load" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand ontology; and __fish_seen_subcommand_from remove" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand ontology; and __fish_seen_subcommand_from remove" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand validate" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand validate" -l shapes -d 'Shapes used for validation or packaging' -r
complete -c qp -n "__fish_qp_using_subcommand validate" -l data -d 'RDF data file to validate' -r
complete -c qp -n "__fish_qp_using_subcommand validate" -l format -d 'Output or input format for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand validate" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand repl" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand repl" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand export" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand export" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand export" -l group-id -d 'Select facts belonging to this group' -r
complete -c qp -n "__fish_qp_using_subcommand export" -l construct -d 'SPARQL CONSTRUCT query selecting the share' -r
complete -c qp -n "__fish_qp_using_subcommand export" -l format -d 'Output or input format for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand export" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand render" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l dest -d 'Render output destination' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l valid-at -d 'Valid-time instant to query' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l server -d 'Render service URL' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l query -d 'Query used for rendering' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l template -d 'Render template' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l layout -d 'Render layout' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l path -d 'Relative output path template' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l dry-run -d 'Preview the operation without applying it'
complete -c qp -n "__fish_qp_using_subcommand render" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand stats" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand stats" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand prime" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand prime" -l section -d 'Agent primer section to print' -r
complete -c qp -n "__fish_qp_using_subcommand prime" -l brief -d 'Print the short agent primer'
complete -c qp -n "__fish_qp_using_subcommand prime" -l static -d 'Print the embedded primer without live context'
complete -c qp -n "__fish_qp_using_subcommand prime" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand prime" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l mode -d 'Operation mode' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l limit -d 'Maximum number of results' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l alpha -d 'Hybrid search semantic/keyword weighting' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l rrf-k -d 'Reciprocal-rank fusion constant' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l decay -d 'Search recency decay factor' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l anchor -d 'Entity IRI anchoring graph-aware search' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l anchor-mode -d 'How the anchor affects candidate selection' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l direction -d 'Traversal direction for graph-aware search' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l max-hops -d 'Maximum graph-search traversal hops' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l via -d 'Predicate or path used for traversal' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l fusion -d 'Search result fusion strategy' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l valid-at -d 'Valid-time instant to query' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l type -d 'Entity type IRI to select' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l group -d 'Group identifier for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l structured-query -d 'Structured search query as JSON' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l filters -d 'Structured search filters as JSON' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l graphs -d 'Explicit graph selection for search' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l explain -d 'Include search ranking explanations'
complete -c qp -n "__fish_qp_using_subcommand search" -l all-graphs -d 'Search all permitted graphs explicitly'
complete -c qp -n "__fish_qp_using_subcommand search" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -l kind -d 'Index or graph kind to select' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -l batch-size -d 'Number of entries per indexing batch' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -f -a "status" -d 'Report the state of a search index'
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -f -a "backfill" -d 'Backfill a search index in batches'
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -f -a "drop" -d 'Drop a search index'
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from status" -l kind -d 'Index or graph kind to select' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from backfill" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from backfill" -l kind -d 'Index or graph kind to select' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from backfill" -l batch-size -d 'Number of entries per indexing batch' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from backfill" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from drop" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from drop" -l kind -d 'Index or graph kind to select' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from drop" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand doctor; and not __fish_seen_subcommand_from labels" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand doctor; and not __fish_seen_subcommand_from labels" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand doctor; and not __fish_seen_subcommand_from labels" -f -a "labels" -d 'Report graph-label drift and recommended floors'
complete -c qp -n "__fish_qp_using_subcommand doctor; and __fish_seen_subcommand_from labels" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand doctor; and __fish_seen_subcommand_from labels" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand pack" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l verify -d 'Pack file to verify without creating a new pack' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l out -d 'Output file' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l space -d 'Namespace space recorded in pack metadata' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l name -d 'Name of the generated object' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l version -d 'Version recorded in pack metadata' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l shapes -d 'Shapes used for validation or packaging' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l queries -d 'Stored queries to include' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l repo -d 'Repository identity or source root for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l repo-sha -d 'Repository commit recorded in pack provenance' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l model-id -d 'Embedding model identifier' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l model-version -d 'Embedding model version' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l destination -d 'Publication destination evaluated by policy' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l format -d 'Output or input format for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l with-vectors -d 'Include embedding vectors in the pack'
complete -c qp -n "__fish_qp_using_subcommand pack" -l full -d 'Include the full store or full primer, as applicable'
complete -c qp -n "__fish_qp_using_subcommand pack" -l allow-missing-embedding-recipe -d 'Allow a pack without the embedding provider recipe'
complete -c qp -n "__fish_qp_using_subcommand pack" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand unpack" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand unpack" -l into -d 'Target store for unpacking' -r
complete -c qp -n "__fish_qp_using_subcommand unpack" -l expect-repo -d 'Expected repository identity in the pack' -r
complete -c qp -n "__fish_qp_using_subcommand unpack" -l head-sha -d 'Expected repository head commit' -r
complete -c qp -n "__fish_qp_using_subcommand unpack" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand restore" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand restore" -l force -d 'Permit replacement when restoring a store'
complete -c qp -n "__fish_qp_using_subcommand restore" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l output -d 'Output directory for the share' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l group-id -d 'Select facts belonging to this group' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l construct -d 'SPARQL CONSTRUCT query selecting the share' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l shapes -d 'Shapes used for validation or packaging' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l queries -d 'Stored queries to include' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l alignments -d 'Reviewed SSSOM/RDF alignment file to carry' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l parent-share -d 'Parent share for a delta export' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l since -d 'Starting share or transaction for incremental output' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l destination -d 'Publication destination evaluated by policy' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l pack-dir -d 'Existing pack directory to share' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-key -d 'Ed25519 attestation signing key file' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-issued-at -d 'Attestation issue time as Unix seconds' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-ttl -d 'Attestation lifetime in seconds' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-agent -d 'Attested agent identifier' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-session -d 'Attested session identifier' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-introducer -d 'Attestation introducer identifier' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-nonce -d 'Unique attestation nonce' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l pointers -d 'TOML file declaring external graph pointers' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l no-shapes -d 'Omit shapes from the share'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l no-queries -d 'Omit stored queries from the share'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l turtle -d 'Include a human-readable Turtle export'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest -d 'Attach a signed agent attestation'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l project -d 'Export this repository\'s project graph to .quipu/graph'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -f -a "diff" -d 'Diff two packs by entity, or write a PR-review report'
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l format -d 'Output or input format for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l old-shapes -d 'Shapes used to validate the old share' -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l new-shapes -d 'Shapes used to validate the new share' -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l decisions -d 'Conflict decisions file' -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l report -d 'Print the validation diff report'
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l fail-on-introduced -d 'Exit nonzero when the diff introduces violations'
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l emit-decisions -d 'Write merge decisions to this file' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l decisions -d 'Conflict decisions file' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l reviewer -d 'Identity reviewing the merge' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l slot-scoped -d 'Restrict the merge to selected slots'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l propose -d 'Produce a merge proposal without applying it'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l dry-run -d 'Preview the operation without applying it'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -a "show" -d 'Show a merge\'s conflicts in the standard layout'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -a "resolve" -d 'Record one choice on one merge conflict'
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from show" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from show" -l format -d 'Output or input format for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -l choose -d 'Conflict side to choose' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -l value -d 'Explicit value for a conflict resolution' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -l by -d 'Identity recording a resolution' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l source -d 'Source identifier recorded in provenance' -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l destination -d 'Publication destination evaluated by policy' -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l query-namespace -d 'Namespace for imported stored queries' -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l replace-queries -d 'Replace imported query definitions on name conflicts'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -a "delta" -d 'Verify a delta share against its parent share'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -a "promote" -d 'Promote a staged share into the store'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -a "review" -d 'Review staged shares awaiting promotion'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -l destination -d 'Publication destination evaluated by policy' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -l query-namespace -d 'Namespace for imported stored queries' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from promote" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from promote" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from promote" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "pending" -d 'List staged shares awaiting review'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "notify" -d 'Report staged shares older than an age threshold to a route'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "rejected" -d 'Mark a staged share rejected, with a reason'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "expired" -d 'Mark a staged share expired, with a reason'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "reopen" -d 'Reopen a rejected or expired staged share'
complete -c qp -n "__fish_qp_using_subcommand compose" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l repo -d 'Repository identity or source root for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l shapes-from -d 'Pack supplying the composition shapes' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l destination -d 'Publication destination evaluated by policy' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l revoke-tombstone -d 'Tombstone triple to revoke in the composition' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l pack-key -d 'Composition pack key' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l reason -d 'Reason recorded for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l use-pointer -d 'Pointer to use in the composition' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l declaration -d 'Composition declaration file' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l authority -d 'Authority identifier for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l repo-remote -d 'Repository remote used to resolve a pointer' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l pointer-cache -d 'Local directory for resolved pack pointers' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l rollback -d 'Restore the previous CURRENT composition snapshot'
complete -c qp -n "__fish_qp_using_subcommand compose" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand changes" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand changes" -l from -d 'First transaction to include' -r
complete -c qp -n "__fish_qp_using_subcommand changes" -l capture -d 'Change-feed row capture mode' -r
complete -c qp -n "__fish_qp_using_subcommand changes" -l limit -d 'Maximum number of results' -r
complete -c qp -n "__fish_qp_using_subcommand changes" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand changes" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand follow" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand follow" -l primary -d 'Primary server URL to follow' -r
complete -c qp -n "__fish_qp_using_subcommand follow" -l page-txs -d 'Transactions per follower page' -r
complete -c qp -n "__fish_qp_using_subcommand follow" -l page-gap-ms -d 'Delay between follower pages in milliseconds' -r
complete -c qp -n "__fish_qp_using_subcommand follow" -l bootstrap-only -d 'Bootstrap a follower and exit without polling'
complete -c qp -n "__fish_qp_using_subcommand follow" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand acquire; and not __fish_seen_subcommand_from scope" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and not __fish_seen_subcommand_from scope" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand acquire; and not __fish_seen_subcommand_from scope" -f -a "scope" -d 'Report the bounded row and byte scope of a direct-main acquisition'
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l source-store-id -d 'Identity of the source store being acquired' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l tx -d 'Historical transaction to read' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l max-vm-steps -d 'Maximum SPARQL virtual-machine steps' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l deadline-ms -d 'Acquisition query deadline in milliseconds' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -l from -d 'Source vector backend' -r
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -l to -d 'Destination vector backend' -r
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -l dry-run -d 'Preview the operation without applying it'
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and not __fish_seen_subcommand_from show resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and not __fish_seen_subcommand_from show resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and not __fish_seen_subcommand_from show resolve" -a "show" -d 'Show a qpack\'s pending merge decisions in the standard layout'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and not __fish_seen_subcommand_from show resolve" -a "resolve" -d 'Record one choice in the Git merge in progress and rewrite the qpack'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from show" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from show" -l format -d 'Output or input format for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from resolve" -l choose -d 'Conflict side to choose' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from resolve" -l by -d 'Identity recording a resolution' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand merge-driver" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand merge-driver" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand pendant-resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand pendant-resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand pendant-check" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand pendant-check" -l history -d 'Check the pendant merge history'
complete -c qp -n "__fish_qp_using_subcommand pendant-check" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand diff-textconv" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand diff-textconv" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand mcp" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand mcp" -l mcp-token-file -d 'File containing the MCP authentication token' -r
complete -c qp -n "__fish_qp_using_subcommand mcp" -l bind -d 'MCP HTTP listen address' -r
complete -c qp -n "__fish_qp_using_subcommand mcp" -l primary-url -d 'Primary server URL used by a read-only MCP follower' -r
complete -c qp -n "__fish_qp_using_subcommand mcp" -l read-only -d 'Serve MCP in read-only mode'
complete -c qp -n "__fish_qp_using_subcommand mcp" -l embed-backfill -d 'Enable embedding backfill in the MCP server'
complete -c qp -n "__fish_qp_using_subcommand mcp" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hook; and not __fish_seen_subcommand_from session-capture" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hook; and not __fish_seen_subcommand_from session-capture" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hook; and not __fish_seen_subcommand_from session-capture" -f -a "session-capture" -d 'Stop hook: solicit one knowledge episode per session from stdin JSON'
complete -c qp -n "__fish_qp_using_subcommand hook; and __fish_seen_subcommand_from session-capture" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hook; and __fish_seen_subcommand_from session-capture" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hooks; and not __fish_seen_subcommand_from bundle install uninstall status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and not __fish_seen_subcommand_from bundle install uninstall status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hooks; and not __fish_seen_subcommand_from bundle install uninstall status" -f -a "bundle" -d 'Print quipu\'s hook bundle (st.hook-bundle/1)'
complete -c qp -n "__fish_qp_using_subcommand hooks; and not __fish_seen_subcommand_from bundle install uninstall status" -f -a "install" -d 'Install quipu\'s hooks into Claude Code or Codex'
complete -c qp -n "__fish_qp_using_subcommand hooks; and not __fish_seen_subcommand_from bundle install uninstall status" -f -a "uninstall" -d 'Remove quipu\'s hooks from Claude Code or Codex'
complete -c qp -n "__fish_qp_using_subcommand hooks; and not __fish_seen_subcommand_from bundle install uninstall status" -f -a "status" -d 'Report whether quipu\'s hooks are installed'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from bundle" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from bundle" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -l harness -d 'Agent harness: claude or codex' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -l project -d 'Use this repository\'s project graph or project-local hooks'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -l no-st -d 'Do not integrate hooks with the crew orchestrator'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -l harness -d 'Agent harness: claude or codex' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -l project -d 'Use this repository\'s project graph or project-local hooks'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -l no-st -d 'Do not integrate hooks with the crew orchestrator'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -l harness -d 'Agent harness: claude or codex' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -l project -d 'Use this repository\'s project graph or project-local hooks'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -l no-st -d 'Do not integrate hooks with the crew orchestrator'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand policy; and not __fish_seen_subcommand_from draft backtest" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and not __fish_seen_subcommand_from draft backtest" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand policy; and not __fish_seen_subcommand_from draft backtest" -f -a "draft" -d 'Draft advisory policy Turtle from an exemplar and an intent'
complete -c qp -n "__fish_qp_using_subcommand policy; and not __fish_seen_subcommand_from draft backtest" -f -a "backtest" -d 'Backtest a candidate policy over recorded transactions'
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l exemplar -d 'Source exemplar for a policy draft' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l name -d 'Name of the generated object' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l label -d 'Human-readable label for the draft' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l targets -d 'Target class IRI for the policy' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l claim -d 'ASK query expressing the proposed policy claim' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l class -d 'Policy classification: soft or hard' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l point -d 'Policy verification point' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l layer -d 'Policy enforcement layer' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l authority -d 'Authority identifier for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l out -d 'Output file' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -l last-txs -d 'Number of recent transactions to evaluate' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -l from-tx -d 'First transaction in the selected interval' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -l to-tx -d 'Last transaction in the selected interval' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand gate; and not __fish_seen_subcommand_from shadow" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and not __fish_seen_subcommand_from shadow" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand gate; and not __fish_seen_subcommand_from shadow" -f -a "shadow" -d 'Judge a candidate policy set over recorded history on a quiescent copy'
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l rules -d 'Rule set to load or evaluate' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l since -d 'Starting share or transaction for incremental output' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l from-tx -d 'First transaction in the selected interval' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l to-tx -d 'Last transaction in the selected interval' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l last-txs -d 'Number of recent transactions to evaluate' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l mode -d 'Operation mode' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l max-txs -d 'Maximum transactions to evaluate' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -f -a "cone" -d 'Compute the provenance cone of a trajectory'
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -f -a "backtest" -d 'Backtest a pruned trajectory over recorded history'
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -f -a "draft" -d 'Draft a golden-path policy from a trajectory as Turtle for review'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -l via -d 'Predicate or path used for traversal' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -l hops -d 'Maximum traversal hops' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from backtest" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from backtest" -l omit -d 'Trajectory step to omit in a counterfactual' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from backtest" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from backtest" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l name -d 'Name of the generated object' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l label -d 'Human-readable label for the draft' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l via -d 'Predicate or path used for traversal' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l omit -d 'Trajectory step to omit in a counterfactual' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l by -d 'Identity recording a resolution' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l dead-end -d 'Dead-end trajectory to include in the policy draft' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -f -a "propose" -d 'Propose scored alignments between two named graphs as an SSSOM set'
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -f -a "decide" -d 'Apply reviewer accept and negate decisions to a mapping set'
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -f -a "apply" -d 'Materialize decided alignments into a derived alignment graph'
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from propose" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from propose" -l set-id -d 'Identifier for the proposed alignment set' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from propose" -l out -d 'Output file' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from propose" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -l decisions -d 'Conflict decisions file' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -l reviewer -d 'Identity reviewing the merge' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -l out -d 'Output file' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l graph-a -d 'First graph in the alignment pair' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l graph-b -d 'Second graph in the alignment pair' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l expected-version -d 'Expected version for the compare-and-swap operation' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -l status -d 'Status to record or filter' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "list" -d 'List schema change proposals, optionally by status'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "submit" -d 'Submit a schema change proposal'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "plan" -d 'Report a pending proposal\'s blast radius and 0-100 risk score without writing; exit 0 pass, 1 over threshold, 2 not determined'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "accept" -d 'Accept a pending proposal; refused when its plan is over the risk threshold unless --accept-risk --reason'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "reject" -d 'Reject a pending proposal'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from list" -l status -d 'Status to record or filter' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -l proposer -d 'Identity proposing an alignment' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -l rationale -d 'Reason for the alignment decision' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -l trigger -d 'Trigger identifier' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from plan" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from plan" -l format -d 'Plan output format: text or json' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from plan" -l risk-threshold -d 'Fail the plan above this 0-100 risk score (default: the deployment threshold)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from plan" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l decided-by -d 'Identity recording the alignment decision' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l note -d 'Explanatory note recorded with the operation' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l reason -d 'Why an over-threshold plan is accepted; recorded with the decision' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l risk-threshold -d 'Refuse acceptance above this 0-100 risk score (default: the deployment threshold)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l accept-risk -d 'Accept despite a plan over the risk threshold; requires --reason'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from reject" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from reject" -l decided-by -d 'Identity recording the alignment decision' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from reject" -l note -d 'Explanatory note recorded with the operation' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from reject" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -f -a "read" -d 'Read a versioned memory file for an owner'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -f -a "export" -d 'Export an owner\'s memory files as a file-data envelope'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -f -a "receipt" -d 'Compute the receipt a memory commit request would produce'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -f -a "commit" -d 'Commit a versioned memory file change'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from read" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from read" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from read" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from export" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from export" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from export" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from receipt" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from receipt" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from receipt" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from commit" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from commit" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from commit" -l local-authority -d 'Explicitly authorize a local memory commit'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from commit" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -f -a "register" -d 'Register a session binding for attested writes'
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -f -a "list" -d 'List registered session bindings'
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -f -a "allow-write" -d 'Allow a session to write'
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -f -a "deny-write" -d 'Stop a session from writing'
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -f -a "require-v2" -d 'Require write-v2 signatures for a session'
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -f -a "allow-v1" -d 'Allow write-v1 signatures for a session again'
complete -c qp -n "__fish_qp_using_subcommand attest; and not __fish_seen_subcommand_from register list allow-write deny-write require-v2 allow-v1 revoke" -f -a "revoke" -d 'Revoke a session binding'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l agent -d 'Agent identifier for the binding' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l session -d 'Session identifier on the statement' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l public-key -d 'Verifier public key' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l introducer -d 'Identifier of the binding introducer' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l issued-at -d 'Issue time as Unix seconds' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l expires-at -d 'Expiration time as Unix seconds' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l allow-write -d 'Grant write permission to the attested binding'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l require-v2 -d 'Require version 2 attestations'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from allow-write" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from allow-write" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from deny-write" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from deny-write" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from require-v2" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from require-v2" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from allow-v1" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from allow-v1" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from revoke" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from revoke" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand signer; and not __fish_seen_subcommand_from enroll status enable disable revoke protect-graph" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and not __fish_seen_subcommand_from enroll status enable disable revoke protect-graph" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand signer; and not __fish_seen_subcommand_from enroll status enable disable revoke protect-graph" -f -a "enroll" -d 'Enroll a hardware signer credential'
complete -c qp -n "__fish_qp_using_subcommand signer; and not __fish_seen_subcommand_from enroll status enable disable revoke protect-graph" -f -a "status" -d 'Show signer enrollment and enforcement state'
complete -c qp -n "__fish_qp_using_subcommand signer; and not __fish_seen_subcommand_from enroll status enable disable revoke protect-graph" -f -a "enable" -d 'Enable signer enforcement'
complete -c qp -n "__fish_qp_using_subcommand signer; and not __fish_seen_subcommand_from enroll status enable disable revoke protect-graph" -f -a "disable" -d 'Disable signer enforcement'
complete -c qp -n "__fish_qp_using_subcommand signer; and not __fish_seen_subcommand_from enroll status enable disable revoke protect-graph" -f -a "revoke" -d 'Revoke a signer credential'
complete -c qp -n "__fish_qp_using_subcommand signer; and not __fish_seen_subcommand_from enroll status enable disable revoke protect-graph" -f -a "protect-graph" -d 'Permanently require attested writes for a graph'
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l credential-id -d 'Base64url-encoded WebAuthn credential ID' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l cose-key -d 'Base64url-encoded COSE public key' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l scheme -d 'Signature scheme' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l origin -d 'WebAuthn origin URL' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l rp-id -d 'WebAuthn relying-party ID' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l expires-at -d 'Expiration time as Unix seconds' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enable" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enable" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from disable" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from disable" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from revoke" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from revoke" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from protect-graph" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from protect-graph" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand deck; and not __fish_seen_subcommand_from configure enroll enable disable revoke status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and not __fish_seen_subcommand_from configure enroll enable disable revoke status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand deck; and not __fish_seen_subcommand_from configure enroll enable disable revoke status" -f -a "configure" -d 'Configure the Decision Deck scope (starts disabled)'
complete -c qp -n "__fish_qp_using_subcommand deck; and not __fish_seen_subcommand_from configure enroll enable disable revoke status" -f -a "enroll" -d 'Enroll a Decision Deck credential'
complete -c qp -n "__fish_qp_using_subcommand deck; and not __fish_seen_subcommand_from configure enroll enable disable revoke status" -f -a "enable" -d 'Enable the Decision Deck'
complete -c qp -n "__fish_qp_using_subcommand deck; and not __fish_seen_subcommand_from configure enroll enable disable revoke status" -f -a "disable" -d 'Disable the Decision Deck'
complete -c qp -n "__fish_qp_using_subcommand deck; and not __fish_seen_subcommand_from configure enroll enable disable revoke status" -f -a "revoke" -d 'Revoke a Decision Deck credential'
complete -c qp -n "__fish_qp_using_subcommand deck; and not __fish_seen_subcommand_from configure enroll enable disable revoke status" -f -a "status" -d 'Show Decision Deck configuration and state'
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l origin -d 'WebAuthn origin URL' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l rp-id -d 'WebAuthn relying-party ID' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l author -d 'Author identifier on the signed statement' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l credential-id -d 'Base64url-encoded WebAuthn credential ID' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l cose-key -d 'Base64url-encoded COSE public key' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l scheme -d 'Signature scheme' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l expires-at -d 'Expiration time as Unix seconds' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enable" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enable" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from disable" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from disable" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from revoke" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from revoke" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand trust-root; and not __fish_seen_subcommand_from challenge bootstrap status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and not __fish_seen_subcommand_from challenge bootstrap status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand trust-root; and not __fish_seen_subcommand_from challenge bootstrap status" -f -a "challenge" -d 'Issue the proof-of-possession challenge for a first verifier key'
complete -c qp -n "__fish_qp_using_subcommand trust-root; and not __fish_seen_subcommand_from challenge bootstrap status" -f -a "bootstrap" -d 'Enroll the first verifier key with its signed challenge'
complete -c qp -n "__fish_qp_using_subcommand trust-root; and not __fish_seen_subcommand_from challenge bootstrap status" -f -a "status" -d 'Show the root-of-trust state'
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from challenge" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from challenge" -l verifier -d 'Verifier identifier for a signed statement' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from challenge" -l public-key -d 'Verifier public key' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from challenge" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l verifier -d 'Verifier identifier for a signed statement' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l public-key -d 'Verifier public key' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l pop-signature -d 'Proof-of-possession signature' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l attests -d 'IRI of the object being attested' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "create" -d 'Register a committed named graph'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "label" -d 'Declare lattice labels on a registered graph'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "list" -d 'List registered named graphs'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "import" -d 'Import another store file as a named graph'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "freeze" -d 'Freeze a graph\'s full history into a read-only archive pack'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "thaw" -d 'Restore a frozen graph from its archive pack'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "overlay-create" -d 'Create an overlay graph bound to a committed parent'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "overlay-write" -d 'Assert, retract or tombstone a triple in an overlay'
complete -c qp -n "__fish_qp_using_subcommand graph; and not __fish_seen_subcommand_from create label list import freeze thaw overlay-create overlay-write overlay-compose" -f -a "overlay-compose" -d 'Resolve an overlay\'s composed view over its parent'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from create" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from create" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from create" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from create" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -l label -d 'Human-readable label for the draft' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l kind -d 'Index or graph kind to select' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l frozen -d 'List only frozen graphs'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from import" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from import" -l as -d 'IRI under which to import the graph' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from import" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from freeze" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from freeze" -l out -d 'Output file' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from freeze" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from freeze" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from thaw" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from thaw" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from thaw" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-create" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-create" -l input -d 'Input file for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-create" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-write" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-write" -l input -d 'Input file for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-write" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-compose" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-compose" -l input -d 'Input file for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-compose" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -f -a "respace" -d 'Copy the store into a term space'
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -f -a "attach" -d 'List the databases mounted alongside this store'
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -f -a "migrate-inferred" -d 'Move engine-derived facts into their companion inferred graphs'
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from respace" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from respace" -l into -d 'New integer namespace space number' -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from respace" -l out -d 'Output file' -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from respace" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from attach" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from attach" -l list -d 'List mounted read-only databases'
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from attach" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from migrate-inferred" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from migrate-inferred" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand events; and not __fish_seen_subcommand_from refusals" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand events; and not __fish_seen_subcommand_from refusals" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand events; and not __fish_seen_subcommand_from refusals" -f -a "refusals" -d 'List recorded write refusals'
complete -c qp -n "__fish_qp_using_subcommand events; and __fish_seen_subcommand_from refusals" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand events; and __fish_seen_subcommand_from refusals" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -l name -d 'Name of the generated object' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -a "list" -d 'List named forks'
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -a "diff" -d 'Diff two forks'
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -a "drop" -d 'Drop a named fork'
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -a "promote" -d 'Promote a fork to ROOT, with reviewable conflict decisions'
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from diff" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from diff" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from drop" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from drop" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l timestamp -d 'Valid-time timestamp for the operation' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l emit-decisions -d 'Write merge decisions to this file' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l decisions -d 'Conflict decisions file' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l reviewer -d 'Identity reviewing the merge' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l actor -d 'Actor recorded in operation provenance' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l propose -d 'Produce a merge proposal without applying it'
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l dry-run -d 'Preview the operation without applying it'
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l repo -d 'Repository identity or source root for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l from -d 'Base Git revision for the audit' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l to -d 'Tip Git revision for the audit' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l yupana -d 'Yupana executable used for the audit' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "inventory" -d 'List the policies the audit checks against'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "namespace" -d 'Report base-namespace drift'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "replay" -d 'Re-derive a gate decision as of its transaction'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "tree" -d 'Reassemble the dispatch forest from a trace\'s principal chains'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "inheritance" -d 'Check whether inherited constraints stopped applying under delegation'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "quarantine" -d 'List refused-attempt evidence behind verdicts'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inventory" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inventory" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inventory" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from namespace" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from namespace" -l graph -d 'Explicit graph IRI for this operation' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from namespace" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from namespace" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from replay" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from replay" -l delta -d 'Refused-delta file for verdict replay' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from replay" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from replay" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from tree" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from tree" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from tree" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inheritance" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inheritance" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inheritance" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -l verdict -d 'Verdict identifier to inspect' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -l json -d 'Print machine-readable JSON'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -f -a "list" -d 'List refused-attempt evidence, optionally for one verdict'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -f -a "purge" -d 'Purge quarantined evidence by graph, age or all'
