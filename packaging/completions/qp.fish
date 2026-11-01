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
complete -c qp -n "__fish_qp_using_subcommand install-alias" -l bin-dir -r
complete -c qp -n "__fish_qp_using_subcommand install-alias" -l check
complete -c qp -n "__fish_qp_using_subcommand install-alias" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand knot" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand knot" -l shapes -r
complete -c qp -n "__fish_qp_using_subcommand knot" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand knot" -l timestamp -r
complete -c qp -n "__fish_qp_using_subcommand knot" -l blank-node-scope -r
complete -c qp -n "__fish_qp_using_subcommand knot" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand load" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand load" -l shapes -r
complete -c qp -n "__fish_qp_using_subcommand load" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand load" -l timestamp -r
complete -c qp -n "__fish_qp_using_subcommand load" -l blank-node-scope -r
complete -c qp -n "__fish_qp_using_subcommand load" -l destination -r
complete -c qp -n "__fish_qp_using_subcommand load" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand load" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand ingest" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l timestamp -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l declare-count -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l declare-sha256 -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l format -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l chunk -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l base-iri -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l source -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -l blank-node-scope -r
complete -c qp -n "__fish_qp_using_subcommand ingest" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand read" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand read" -l valid-at -r
complete -c qp -n "__fish_qp_using_subcommand read" -l tx -r
complete -c qp -n "__fish_qp_using_subcommand read" -l fork -r
complete -c qp -n "__fish_qp_using_subcommand read" -l entailment -r
complete -c qp -n "__fish_qp_using_subcommand read" -l federation
complete -c qp -n "__fish_qp_using_subcommand read" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand query" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand query" -l valid-at -r
complete -c qp -n "__fish_qp_using_subcommand query" -l tx -r
complete -c qp -n "__fish_qp_using_subcommand query" -l fork -r
complete -c qp -n "__fish_qp_using_subcommand query" -l entailment -r
complete -c qp -n "__fish_qp_using_subcommand query" -l federation
complete -c qp -n "__fish_qp_using_subcommand query" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand rule-query" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand rule-query" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand cord" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand cord" -l type -r
complete -c qp -n "__fish_qp_using_subcommand cord" -l limit -r
complete -c qp -n "__fish_qp_using_subcommand cord" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand unravel" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand unravel" -l tx -r
complete -c qp -n "__fish_qp_using_subcommand unravel" -l valid-at -r
complete -c qp -n "__fish_qp_using_subcommand unravel" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand impact" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand impact" -l hops -r
complete -c qp -n "__fish_qp_using_subcommand impact" -l predicate -r
complete -c qp -n "__fish_qp_using_subcommand impact" -l remove
complete -c qp -n "__fish_qp_using_subcommand impact" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand explain" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand explain" -l depth -r
complete -c qp -n "__fish_qp_using_subcommand explain" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand project" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand project" -l algorithm -r
complete -c qp -n "__fish_qp_using_subcommand project" -l type -r
complete -c qp -n "__fish_qp_using_subcommand project" -l predicate -r
complete -c qp -n "__fish_qp_using_subcommand project" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand project" -l limit -r
complete -c qp -n "__fish_qp_using_subcommand project" -l damping -r
complete -c qp -n "__fish_qp_using_subcommand project" -l max-iters -r
complete -c qp -n "__fish_qp_using_subcommand project" -l tolerance -r
complete -c qp -n "__fish_qp_using_subcommand project" -l from -r
complete -c qp -n "__fish_qp_using_subcommand project" -l to -r
complete -c qp -n "__fish_qp_using_subcommand project" -l seed -r
complete -c qp -n "__fish_qp_using_subcommand project" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand report" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand report" -l type -r
complete -c qp -n "__fish_qp_using_subcommand report" -l predicate -r
complete -c qp -n "__fish_qp_using_subcommand report" -l hubs -r
complete -c qp -n "__fish_qp_using_subcommand report" -l surprises -r
complete -c qp -n "__fish_qp_using_subcommand report" -l questions -r
complete -c qp -n "__fish_qp_using_subcommand report" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand reason" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand reason" -l rules -r
complete -c qp -n "__fish_qp_using_subcommand reason" -l reactive
complete -c qp -n "__fish_qp_using_subcommand reason" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand demotions" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand demotions" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand demotions" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand episode" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand episode" -l base-ns -r
complete -c qp -n "__fish_qp_using_subcommand episode" -l timestamp -r
complete -c qp -n "__fish_qp_using_subcommand episode" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand retract" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand retract" -l predicate -r
complete -c qp -n "__fish_qp_using_subcommand retract" -l timestamp -r
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
complete -c qp -n "__fish_qp_using_subcommand validate" -l shapes -r
complete -c qp -n "__fish_qp_using_subcommand validate" -l data -r
complete -c qp -n "__fish_qp_using_subcommand validate" -l format -r
complete -c qp -n "__fish_qp_using_subcommand validate" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand repl" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand repl" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand export" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand export" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand export" -l group-id -r
complete -c qp -n "__fish_qp_using_subcommand export" -l construct -r
complete -c qp -n "__fish_qp_using_subcommand export" -l format -r
complete -c qp -n "__fish_qp_using_subcommand export" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand render" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand render" -l dest -r
complete -c qp -n "__fish_qp_using_subcommand render" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand render" -l valid-at -r
complete -c qp -n "__fish_qp_using_subcommand render" -l server -r
complete -c qp -n "__fish_qp_using_subcommand render" -l query -r
complete -c qp -n "__fish_qp_using_subcommand render" -l template -r
complete -c qp -n "__fish_qp_using_subcommand render" -l layout -r
complete -c qp -n "__fish_qp_using_subcommand render" -l path -r
complete -c qp -n "__fish_qp_using_subcommand render" -l dry-run
complete -c qp -n "__fish_qp_using_subcommand render" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand stats" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand stats" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand prime" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand prime" -l section -r
complete -c qp -n "__fish_qp_using_subcommand prime" -l brief
complete -c qp -n "__fish_qp_using_subcommand prime" -l static
complete -c qp -n "__fish_qp_using_subcommand prime" -l json
complete -c qp -n "__fish_qp_using_subcommand prime" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search" -l mode -r
complete -c qp -n "__fish_qp_using_subcommand search" -l limit -r
complete -c qp -n "__fish_qp_using_subcommand search" -l alpha -r
complete -c qp -n "__fish_qp_using_subcommand search" -l rrf-k -r
complete -c qp -n "__fish_qp_using_subcommand search" -l decay -r
complete -c qp -n "__fish_qp_using_subcommand search" -l anchor -r
complete -c qp -n "__fish_qp_using_subcommand search" -l anchor-mode -r
complete -c qp -n "__fish_qp_using_subcommand search" -l direction -r
complete -c qp -n "__fish_qp_using_subcommand search" -l max-hops -r
complete -c qp -n "__fish_qp_using_subcommand search" -l via -r
complete -c qp -n "__fish_qp_using_subcommand search" -l fusion -r
complete -c qp -n "__fish_qp_using_subcommand search" -l valid-at -r
complete -c qp -n "__fish_qp_using_subcommand search" -l type -r
complete -c qp -n "__fish_qp_using_subcommand search" -l group -r
complete -c qp -n "__fish_qp_using_subcommand search" -l structured-query -r
complete -c qp -n "__fish_qp_using_subcommand search" -l filters -r
complete -c qp -n "__fish_qp_using_subcommand search" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand search" -l graphs -r
complete -c qp -n "__fish_qp_using_subcommand search" -l explain
complete -c qp -n "__fish_qp_using_subcommand search" -l all-graphs
complete -c qp -n "__fish_qp_using_subcommand search" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -l kind -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -l batch-size -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -f -a "status" -d 'Report the state of a search index'
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -f -a "backfill" -d 'Backfill a search index in batches'
complete -c qp -n "__fish_qp_using_subcommand search-index; and not __fish_seen_subcommand_from status backfill drop" -f -a "drop" -d 'Drop a search index'
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from status" -l kind -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from backfill" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from backfill" -l kind -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from backfill" -l batch-size -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from backfill" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from drop" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from drop" -l kind -r
complete -c qp -n "__fish_qp_using_subcommand search-index; and __fish_seen_subcommand_from drop" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand doctor; and not __fish_seen_subcommand_from labels" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand doctor; and not __fish_seen_subcommand_from labels" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand doctor; and not __fish_seen_subcommand_from labels" -f -a "labels" -d 'Report graph-label drift and recommended floors'
complete -c qp -n "__fish_qp_using_subcommand doctor; and __fish_seen_subcommand_from labels" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand doctor; and __fish_seen_subcommand_from labels" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand pack" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l verify -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l out -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l space -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l name -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l version -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l shapes -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l queries -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l repo -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l repo-sha -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l model-id -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l model-version -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l destination -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l format -r
complete -c qp -n "__fish_qp_using_subcommand pack" -l with-vectors
complete -c qp -n "__fish_qp_using_subcommand pack" -l full
complete -c qp -n "__fish_qp_using_subcommand pack" -l allow-missing-embedding-recipe
complete -c qp -n "__fish_qp_using_subcommand pack" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand unpack" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand unpack" -l into -r
complete -c qp -n "__fish_qp_using_subcommand unpack" -l expect-repo -r
complete -c qp -n "__fish_qp_using_subcommand unpack" -l head-sha -r
complete -c qp -n "__fish_qp_using_subcommand unpack" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand restore" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand restore" -l force
complete -c qp -n "__fish_qp_using_subcommand restore" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l output -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l group-id -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l construct -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l shapes -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l queries -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l alignments -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l parent-share -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l since -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l destination -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l pack-dir -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-key -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-issued-at -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-ttl -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-agent -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-session -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-introducer -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest-nonce -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l pointers -r
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l no-shapes
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l no-queries
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l turtle
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l attest
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -l project
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand share; and not __fish_seen_subcommand_from diff" -f -a "diff" -d 'Diff two packs by entity, or write a PR-review report'
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l format -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l old-shapes -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l new-shapes -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l decisions -r
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l report
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -l fail-on-introduced
complete -c qp -n "__fish_qp_using_subcommand share; and __fish_seen_subcommand_from diff" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l emit-decisions -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l decisions -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l reviewer -r
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l slot-scoped
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l propose
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -l dry-run
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -a "show" -d 'Show a merge\'s conflicts in the standard layout'
complete -c qp -n "__fish_qp_using_subcommand merge; and not __fish_seen_subcommand_from show resolve" -a "resolve" -d 'Record one choice on one merge conflict'
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from show" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from show" -l format -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -l choose -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -l value -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -l by -r
complete -c qp -n "__fish_qp_using_subcommand merge; and __fish_seen_subcommand_from resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l source -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l destination -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l query-namespace -r
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -l replace-queries
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -a "delta" -d 'Verify a delta share against its parent share'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -a "promote" -d 'Promote a staged share into the store'
complete -c qp -n "__fish_qp_using_subcommand import; and not __fish_seen_subcommand_from delta promote review" -a "review" -d 'Review staged shares awaiting promotion'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -l destination -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -l query-namespace -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from delta" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from promote" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from promote" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from promote" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "pending" -d 'List staged shares awaiting review'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "notify" -d 'Report staged shares older than an age threshold to a route'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "rejected" -d 'Mark a staged share rejected, with a reason'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "expired" -d 'Mark a staged share expired, with a reason'
complete -c qp -n "__fish_qp_using_subcommand import; and __fish_seen_subcommand_from review" -f -a "reopen" -d 'Reopen a rejected or expired staged share'
complete -c qp -n "__fish_qp_using_subcommand compose" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l repo -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l shapes-from -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l destination -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l revoke-tombstone -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l pack-key -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l reason -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l use-pointer -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l declaration -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l authority -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l repo-remote -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l pointer-cache -r
complete -c qp -n "__fish_qp_using_subcommand compose" -l rollback
complete -c qp -n "__fish_qp_using_subcommand compose" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand changes" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand changes" -l from -r
complete -c qp -n "__fish_qp_using_subcommand changes" -l capture -r
complete -c qp -n "__fish_qp_using_subcommand changes" -l limit -r
complete -c qp -n "__fish_qp_using_subcommand changes" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand changes" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand follow" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand follow" -l primary -r
complete -c qp -n "__fish_qp_using_subcommand follow" -l page-txs -r
complete -c qp -n "__fish_qp_using_subcommand follow" -l page-gap-ms -r
complete -c qp -n "__fish_qp_using_subcommand follow" -l bootstrap-only
complete -c qp -n "__fish_qp_using_subcommand follow" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand acquire; and not __fish_seen_subcommand_from scope" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and not __fish_seen_subcommand_from scope" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand acquire; and not __fish_seen_subcommand_from scope" -f -a "scope" -d 'Report the bounded row and byte scope of a direct-main acquisition'
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l source-store-id -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l tx -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l max-vm-steps -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -l deadline-ms -r
complete -c qp -n "__fish_qp_using_subcommand acquire; and __fish_seen_subcommand_from scope" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -l from -r
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -l to -r
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -l dry-run
complete -c qp -n "__fish_qp_using_subcommand migrate-vectors" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and not __fish_seen_subcommand_from show resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and not __fish_seen_subcommand_from show resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and not __fish_seen_subcommand_from show resolve" -a "show" -d 'Show a qpack\'s pending merge decisions in the standard layout'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and not __fish_seen_subcommand_from show resolve" -a "resolve" -d 'Record one choice in the Git merge in progress and rewrite the qpack'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from show" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from show" -l format -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from resolve" -l choose -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from resolve" -l by -r
complete -c qp -n "__fish_qp_using_subcommand git-merge; and __fish_seen_subcommand_from resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand merge-driver" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand merge-driver" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand pendant-resolve" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand pendant-resolve" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand pendant-check" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand pendant-check" -l history
complete -c qp -n "__fish_qp_using_subcommand pendant-check" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand diff-textconv" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand diff-textconv" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand mcp" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand mcp" -l mcp-token-file -r
complete -c qp -n "__fish_qp_using_subcommand mcp" -l bind -r
complete -c qp -n "__fish_qp_using_subcommand mcp" -l primary-url -r
complete -c qp -n "__fish_qp_using_subcommand mcp" -l read-only
complete -c qp -n "__fish_qp_using_subcommand mcp" -l embed-backfill
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
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -l harness -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -l project
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -l no-st
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from install" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -l harness -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -l project
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -l no-st
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from uninstall" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -l harness -r
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -l project
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -l no-st
complete -c qp -n "__fish_qp_using_subcommand hooks; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand policy; and not __fish_seen_subcommand_from draft backtest" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and not __fish_seen_subcommand_from draft backtest" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand policy; and not __fish_seen_subcommand_from draft backtest" -f -a "draft" -d 'Draft advisory policy Turtle from an exemplar and an intent'
complete -c qp -n "__fish_qp_using_subcommand policy; and not __fish_seen_subcommand_from draft backtest" -f -a "backtest" -d 'Backtest a candidate policy over recorded transactions'
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l exemplar -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l name -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l label -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l targets -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l claim -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l class -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l point -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l layer -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l authority -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -l out -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from draft" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -l last-txs -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -l from-tx -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -l to-tx -r
complete -c qp -n "__fish_qp_using_subcommand policy; and __fish_seen_subcommand_from backtest" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand gate; and not __fish_seen_subcommand_from shadow" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and not __fish_seen_subcommand_from shadow" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand gate; and not __fish_seen_subcommand_from shadow" -f -a "shadow" -d 'Judge a candidate policy set over recorded history on a quiescent copy'
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l rules -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l since -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l from-tx -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l to-tx -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l last-txs -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l mode -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l max-txs -r
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -l json
complete -c qp -n "__fish_qp_using_subcommand gate; and __fish_seen_subcommand_from shadow" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -f -a "cone" -d 'Compute the provenance cone of a trajectory'
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -f -a "backtest" -d 'Backtest a pruned trajectory over recorded history'
complete -c qp -n "__fish_qp_using_subcommand path; and not __fish_seen_subcommand_from cone backtest draft" -f -a "draft" -d 'Draft a golden-path policy from a trajectory as Turtle for review'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -l via -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -l hops -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -l json
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from cone" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from backtest" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from backtest" -l omit -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from backtest" -l json
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from backtest" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l name -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l label -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l via -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l omit -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l by -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l dead-end -r
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -l json
complete -c qp -n "__fish_qp_using_subcommand path; and __fish_seen_subcommand_from draft" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -f -a "propose" -d 'Propose scored alignments between two named graphs as an SSSOM set'
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -f -a "decide" -d 'Apply reviewer accept and negate decisions to a mapping set'
complete -c qp -n "__fish_qp_using_subcommand align; and not __fish_seen_subcommand_from propose decide apply" -f -a "apply" -d 'Materialize decided alignments into a derived alignment graph'
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from propose" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from propose" -l set-id -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from propose" -l out -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from propose" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -l decisions -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -l reviewer -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -l out -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from decide" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l graph-a -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l graph-b -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l expected-version -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand align; and __fish_seen_subcommand_from apply" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -l status -r
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "list" -d 'List schema change proposals, optionally by status'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "submit" -d 'Submit a schema change proposal'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "plan" -d 'Report a pending proposal\'s blast radius and 0-100 risk score without writing; exit 0 pass, 1 over threshold, 2 not determined'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "accept" -d 'Accept a pending proposal; refused when its plan is over the risk threshold unless --accept-risk --reason'
complete -c qp -n "__fish_qp_using_subcommand propose; and not __fish_seen_subcommand_from list submit plan accept reject" -f -a "reject" -d 'Reject a pending proposal'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from list" -l status -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -l proposer -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -l rationale -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -l trigger -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from submit" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from plan" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from plan" -l format -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from plan" -l risk-threshold -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from plan" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l decided-by -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l note -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l reason -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l risk-threshold -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -l accept-risk
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from accept" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from reject" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from reject" -l decided-by -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from reject" -l note -r
complete -c qp -n "__fish_qp_using_subcommand propose; and __fish_seen_subcommand_from reject" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -f -a "read" -d 'Read a versioned memory file for an owner'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -f -a "export" -d 'Export an owner\'s memory files as a file-data envelope'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -f -a "receipt" -d 'Compute the receipt a memory commit request would produce'
complete -c qp -n "__fish_qp_using_subcommand memory; and not __fish_seen_subcommand_from read export receipt commit" -f -a "commit" -d 'Commit a versioned memory file change'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from read" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from read" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from read" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from export" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from export" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from export" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from receipt" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from receipt" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from receipt" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from commit" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from commit" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand memory; and __fish_seen_subcommand_from commit" -l local-authority
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
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l agent -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l session -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l public-key -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l introducer -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l issued-at -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l expires-at -r
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l allow-write
complete -c qp -n "__fish_qp_using_subcommand attest; and __fish_seen_subcommand_from register" -l require-v2
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
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l credential-id -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l cose-key -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l scheme -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l origin -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l rp-id -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand signer; and __fish_seen_subcommand_from enroll" -l expires-at -r
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
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l origin -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l rp-id -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -l author -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from configure" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l credential-id -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l cose-key -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l scheme -r
complete -c qp -n "__fish_qp_using_subcommand deck; and __fish_seen_subcommand_from enroll" -l expires-at -r
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
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from challenge" -l verifier -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from challenge" -l public-key -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from challenge" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l verifier -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l public-key -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l pop-signature -r
complete -c qp -n "__fish_qp_using_subcommand trust-root; and __fish_seen_subcommand_from bootstrap" -l attests -r
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
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from create" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from create" -l timestamp -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from create" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -l label -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -l timestamp -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from label" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l kind -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l frozen
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -l json
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from import" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from import" -l as -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from import" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from freeze" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from freeze" -l out -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from freeze" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from freeze" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from thaw" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from thaw" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from thaw" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-create" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-create" -l input -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-create" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-write" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-write" -l input -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-write" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-compose" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-compose" -l input -r
complete -c qp -n "__fish_qp_using_subcommand graph; and __fish_seen_subcommand_from overlay-compose" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -f -a "respace" -d 'Copy the store into a term space'
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -f -a "attach" -d 'List the databases mounted alongside this store'
complete -c qp -n "__fish_qp_using_subcommand db; and not __fish_seen_subcommand_from respace attach migrate-inferred" -f -a "migrate-inferred" -d 'Move engine-derived facts into their companion inferred graphs'
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from respace" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from respace" -l into -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from respace" -l out -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from respace" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from attach" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from attach" -l list
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from attach" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from migrate-inferred" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand db; and __fish_seen_subcommand_from migrate-inferred" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand events; and not __fish_seen_subcommand_from refusals" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand events; and not __fish_seen_subcommand_from refusals" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand events; and not __fish_seen_subcommand_from refusals" -f -a "refusals" -d 'List recorded write refusals'
complete -c qp -n "__fish_qp_using_subcommand events; and __fish_seen_subcommand_from refusals" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand events; and __fish_seen_subcommand_from refusals" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -l name -r
complete -c qp -n "__fish_qp_using_subcommand fork; and not __fish_seen_subcommand_from list diff drop promote" -l timestamp -r
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
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l timestamp -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l emit-decisions -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l decisions -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l reviewer -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l actor -r
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l propose
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -l dry-run
complete -c qp -n "__fish_qp_using_subcommand fork; and __fish_seen_subcommand_from promote" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l repo -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l from -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l to -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l yupana -r
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -l json
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "inventory" -d 'List the policies the audit checks against'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "namespace" -d 'Report base-namespace drift'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "replay" -d 'Re-derive a gate decision as of its transaction'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "tree" -d 'Reassemble the dispatch forest from a trace\'s principal chains'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "inheritance" -d 'Check whether inherited constraints stopped applying under delegation'
complete -c qp -n "__fish_qp_using_subcommand audit; and not __fish_seen_subcommand_from inventory namespace replay tree inheritance quarantine" -a "quarantine" -d 'List refused-attempt evidence behind verdicts'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inventory" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inventory" -l json
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inventory" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from namespace" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from namespace" -l graph -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from namespace" -l json
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from namespace" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from replay" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from replay" -l delta -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from replay" -l json
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from replay" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from tree" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from tree" -l json
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from tree" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inheritance" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inheritance" -l json
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from inheritance" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -l db -d 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)' -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -l verdict -r
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -l json
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -s h -l help -d 'Print help'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -f -a "list" -d 'List refused-attempt evidence, optionally for one verdict'
complete -c qp -n "__fish_qp_using_subcommand audit; and __fish_seen_subcommand_from quarantine" -f -a "purge" -d 'Purge quarantined evidence by graph, age or all'
