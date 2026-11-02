
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'qp' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'qp'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'qp' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('store', 'store', [CompletionResultType]::ParameterValue, 'Inspect native ownership, or explicitly adopt or restore a disk store')
            [CompletionResult]::new('version', 'version', [CompletionResultType]::ParameterValue, 'Print the version and source commit')
            [CompletionResult]::new('install-alias', 'install-alias', [CompletionResultType]::ParameterValue, 'Add the qp alias beside the quipu binary')
            [CompletionResult]::new('knot', 'knot', [CompletionResultType]::ParameterValue, 'Assert Turtle facts, validated against loaded shapes')
            [CompletionResult]::new('load', 'load', [CompletionResultType]::ParameterValue, 'Assert Turtle facts (alias of knot), or load a project bundle directory')
            [CompletionResult]::new('ingest', 'ingest', [CompletionResultType]::ParameterValue, 'Bulk-load an N-Triples, Turtle or N-Quads file into one graph with declared count and digest')
            [CompletionResult]::new('read', 'read', [CompletionResultType]::ParameterValue, 'Run a SPARQL query, optionally as of a valid time or transaction')
            [CompletionResult]::new('query', 'query', [CompletionResultType]::ParameterValue, 'Run a SPARQL query (alias of read)')
            [CompletionResult]::new('rule-query', 'rule-query', [CompletionResultType]::ParameterValue, 'Read typed ROOT support and the admitted judgment for a rule request')
            [CompletionResult]::new('cord', 'cord', [CompletionResultType]::ParameterValue, 'List entities, optionally of one type')
            [CompletionResult]::new('unravel', 'unravel', [CompletionResultType]::ParameterValue, 'Show facts as they were at a transaction or valid time')
            [CompletionResult]::new('impact', 'impact', [CompletionResultType]::ParameterValue, 'Walk what depends on an entity, optionally after a speculative removal')
            [CompletionResult]::new('explain', 'explain', [CompletionResultType]::ParameterValue, 'Trace a derived fact back to its premises')
            [CompletionResult]::new('project', 'project', [CompletionResultType]::ParameterValue, 'Project the graph and run a graph algorithm such as PageRank')
            [CompletionResult]::new('report', 'report', [CompletionResultType]::ParameterValue, 'Report hubs, surprising links and open questions in the graph')
            [CompletionResult]::new('reason', 'reason', [CompletionResultType]::ParameterValue, 'Run Datalog rules to a fixpoint and persist the derivations')
            [CompletionResult]::new('demotions', 'demotions', [CompletionResultType]::ParameterValue, 'Read preserved unsupported demotions without materializing a closure')
            [CompletionResult]::new('episode', 'episode', [CompletionResultType]::ParameterValue, 'Ingest a structured episode of nodes and edges from JSON')
            [CompletionResult]::new('retract', 'retract', [CompletionResultType]::ParameterValue, 'Retract an entity''s facts, optionally one predicate; history is kept')
            [CompletionResult]::new('shapes', 'shapes', [CompletionResultType]::ParameterValue, 'Load, list or remove the SHACL shapes that guard writes')
            [CompletionResult]::new('namespaces', 'namespaces', [CompletionResultType]::ParameterValue, 'List or apply the owner namespace registry')
            [CompletionResult]::new('entity-ref', 'entity-ref', [CompletionResultType]::ParameterValue, 'Mint or resolve stable entity references')
            [CompletionResult]::new('ontology', 'ontology', [CompletionResultType]::ParameterValue, 'Load, list or remove OWL ontologies (owl feature)')
            [CompletionResult]::new('validate', 'validate', [CompletionResultType]::ParameterValue, 'Validate a data file against a shapes file without writing')
            [CompletionResult]::new('repl', 'repl', [CompletionResultType]::ParameterValue, 'Open an interactive SPARQL prompt')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export RDF from ROOT, one graph, a provenance group or a CONSTRUCT query')
            [CompletionResult]::new('render', 'render', [CompletionResultType]::ParameterValue, 'Render graph data into files in an owned directory')
            [CompletionResult]::new('stats', 'stats', [CompletionResultType]::ParameterValue, 'Show store statistics')
            [CompletionResult]::new('prime', 'prime', [CompletionResultType]::ParameterValue, 'Print the agent primer and a bounded, read-only summary of the store')
            [CompletionResult]::new('search', 'search', [CompletionResultType]::ParameterValue, 'Search entities by keyword, vector or hybrid ranking')
            [CompletionResult]::new('search-index', 'search-index', [CompletionResultType]::ParameterValue, 'Report, backfill or drop the search indexes')
            [CompletionResult]::new('doctor', 'doctor', [CompletionResultType]::ParameterValue, 'Report graph-label drift and recommended floors')
            [CompletionResult]::new('pack', 'pack', [CompletionResultType]::ParameterValue, 'Export a graph, or the whole store, as a pack; or verify a pack')
            [CompletionResult]::new('unpack', 'unpack', [CompletionResultType]::ParameterValue, 'Merge a published pack into a local graph')
            [CompletionResult]::new('restore', 'restore', [CompletionResultType]::ParameterValue, 'Replace the store with a full pack, binary or text')
            [CompletionResult]::new('share', 'share', [CompletionResultType]::ParameterValue, 'Write a portable share of a graph, scope or project')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Report how a share has diverged from this store')
            [CompletionResult]::new('merge', 'merge', [CompletionResultType]::ParameterValue, 'Merge a share back into ROOT, with reviewable conflict decisions')
            [CompletionResult]::new('import', 'import', [CompletionResultType]::ParameterValue, 'Verify a share into quarantine, then promote or review it')
            [CompletionResult]::new('compose', 'compose', [CompletionResultType]::ParameterValue, 'Compose verified packs into a named inspection dataset')
            [CompletionResult]::new('changes', 'changes', [CompletionResultType]::ParameterValue, 'Read whole transaction change pages from the change feed')
            [CompletionResult]::new('follow', 'follow', [CompletionResultType]::ParameterValue, 'Keep this database a read-only follower of a primary server')
            [CompletionResult]::new('acquire', 'acquire', [CompletionResultType]::ParameterValue, 'Measure the bounded scope of a direct-main graph acquisition')
            [CompletionResult]::new('migrate-vectors', 'migrate-vectors', [CompletionResultType]::ParameterValue, 'Move stored vectors between backends (lancedb feature)')
            [CompletionResult]::new('git-merge', 'git-merge', [CompletionResultType]::ParameterValue, 'Merge pendants from Git snapshots and stop before committing')
            [CompletionResult]::new('merge-driver', 'merge-driver', [CompletionResultType]::ParameterValue, 'Low-level Git merge driver for pendant files')
            [CompletionResult]::new('pendant-resolve', 'pendant-resolve', [CompletionResultType]::ParameterValue, 'Record a resolution choice for one pendant merge conflict')
            [CompletionResult]::new('pendant-check', 'pendant-check', [CompletionResultType]::ParameterValue, 'Check a pendant merge result in CI without a merge driver')
            [CompletionResult]::new('diff-textconv', 'diff-textconv', [CompletionResultType]::ParameterValue, 'Render a pack with labels for git diff textconv')
            [CompletionResult]::new('mcp', 'mcp', [CompletionResultType]::ParameterValue, 'Serve the MCP tools over stdio through the sibling quipu-server')
            [CompletionResult]::new('hook', 'hook', [CompletionResultType]::ParameterValue, 'Run a harness hook such as session capture')
            [CompletionResult]::new('hooks', 'hooks', [CompletionResultType]::ParameterValue, 'Bundle, install, uninstall or check quipu''s Claude Code and Codex hooks')
            [CompletionResult]::new('policy', 'policy', [CompletionResultType]::ParameterValue, 'Draft an advisory policy from an exemplar, or backtest one')
            [CompletionResult]::new('gate', 'gate', [CompletionResultType]::ParameterValue, 'Judge a candidate policy set over recorded history without writing')
            [CompletionResult]::new('path', 'path', [CompletionResultType]::ParameterValue, 'Golden-path analysis: provenance cone, backtest and draft')
            [CompletionResult]::new('align', 'align', [CompletionResultType]::ParameterValue, 'Propose, decide and apply cross-graph alignments')
            [CompletionResult]::new('propose', 'propose', [CompletionResultType]::ParameterValue, 'List, submit, plan, accept or reject schema change proposals')
            [CompletionResult]::new('memory', 'memory', [CompletionResultType]::ParameterValue, 'Read, export, receipt or commit versioned file memory')
            [CompletionResult]::new('attest', 'attest', [CompletionResultType]::ParameterValue, 'Register and manage attested session bindings')
            [CompletionResult]::new('signer', 'signer', [CompletionResultType]::ParameterValue, 'Enroll and manage hardware signers for attested writes')
            [CompletionResult]::new('deck', 'deck', [CompletionResultType]::ParameterValue, 'Configure and enroll the Decision Deck')
            [CompletionResult]::new('trust-root', 'trust-root', [CompletionResultType]::ParameterValue, 'Enroll the first human key of the root of trust')
            [CompletionResult]::new('graph', 'graph', [CompletionResultType]::ParameterValue, 'Create, label, list, import, freeze and thaw named graphs and overlays')
            [CompletionResult]::new('db', 'db', [CompletionResultType]::ParameterValue, 'Respace, attach or migrate the store file')
            [CompletionResult]::new('events', 'events', [CompletionResultType]::ParameterValue, 'Read store event records such as refusals')
            [CompletionResult]::new('fork', 'fork', [CompletionResultType]::ParameterValue, 'Create, list, diff, drop or promote named forks of ROOT')
            [CompletionResult]::new('audit', 'audit', [CompletionResultType]::ParameterValue, 'Check traces, namespaces, replays and quarantine against policy')
            break
        }
        'qp;store' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Report inactive or enforced native store ownership without adopting')
            [CompletionResult]::new('adopt', 'adopt', [CompletionResultType]::ParameterValue, 'Bind an existing native store to this host and path with a fresh epoch')
            [CompletionResult]::new('restore', 'restore', [CompletionResultType]::ParameterValue, 'Restore a SQLite snapshot to a new path and explicitly adopt it')
            break
        }
        'qp;store;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;store;adopt' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;store;restore' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;version' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;install-alias' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--bin-dir', '--bin-dir', [CompletionResultType]::ParameterName, 'Directory containing the CLI alias')
            [CompletionResult]::new('--check', '--check', [CompletionResultType]::ParameterName, 'Check the alias without installing it')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;knot' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'Shapes used for validation or packaging')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('--blank-node-scope', '--blank-node-scope', [CompletionResultType]::ParameterName, 'Blank-node identity scope for this import')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;load' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'Shapes used for validation or packaging')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('--blank-node-scope', '--blank-node-scope', [CompletionResultType]::ParameterName, 'Blank-node identity scope for this import')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'Publication destination evaluated by policy')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;ingest' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('--declare-count', '--declare-count', [CompletionResultType]::ParameterName, 'Expected ingest triple count')
            [CompletionResult]::new('--declare-sha256', '--declare-sha256', [CompletionResultType]::ParameterName, 'Expected input SHA-256 digest')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'Output or input format for this operation')
            [CompletionResult]::new('--chunk', '--chunk', [CompletionResultType]::ParameterName, 'Maximum triples per ingest chunk')
            [CompletionResult]::new('--base-iri', '--base-iri', [CompletionResultType]::ParameterName, 'Base IRI for relative RDF references')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--source', '--source', [CompletionResultType]::ParameterName, 'Source identifier recorded in provenance')
            [CompletionResult]::new('--blank-node-scope', '--blank-node-scope', [CompletionResultType]::ParameterName, 'Blank-node identity scope for this import')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;read' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'Valid-time instant to query')
            [CompletionResult]::new('--tx', '--tx', [CompletionResultType]::ParameterName, 'Historical transaction to read')
            [CompletionResult]::new('--fork', '--fork', [CompletionResultType]::ParameterName, 'Named store fork to query')
            [CompletionResult]::new('--entailment', '--entailment', [CompletionResultType]::ParameterName, 'Query entailment regime')
            [CompletionResult]::new('--federation', '--federation', [CompletionResultType]::ParameterName, 'Enable configured remote query federation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;query' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'Valid-time instant to query')
            [CompletionResult]::new('--tx', '--tx', [CompletionResultType]::ParameterName, 'Historical transaction to read')
            [CompletionResult]::new('--fork', '--fork', [CompletionResultType]::ParameterName, 'Named store fork to query')
            [CompletionResult]::new('--entailment', '--entailment', [CompletionResultType]::ParameterName, 'Query entailment regime')
            [CompletionResult]::new('--federation', '--federation', [CompletionResultType]::ParameterName, 'Enable configured remote query federation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;rule-query' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;cord' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--type', '--type', [CompletionResultType]::ParameterName, 'Entity type IRI to select')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'Maximum number of results')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;unravel' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--tx', '--tx', [CompletionResultType]::ParameterName, 'Historical transaction to read')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'Valid-time instant to query')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;impact' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--hops', '--hops', [CompletionResultType]::ParameterName, 'Maximum traversal hops')
            [CompletionResult]::new('--predicate', '--predicate', [CompletionResultType]::ParameterName, 'Predicate IRI used to filter facts or edges')
            [CompletionResult]::new('--remove', '--remove', [CompletionResultType]::ParameterName, 'Calculate the impact of removing the entity')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;explain' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--depth', '--depth', [CompletionResultType]::ParameterName, 'Maximum explanation depth')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;project' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--algorithm', '--algorithm', [CompletionResultType]::ParameterName, 'Graph projection algorithm')
            [CompletionResult]::new('--type', '--type', [CompletionResultType]::ParameterName, 'Entity type IRI to select')
            [CompletionResult]::new('--predicate', '--predicate', [CompletionResultType]::ParameterName, 'Predicate IRI used to filter facts or edges')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'Maximum number of results')
            [CompletionResult]::new('--damping', '--damping', [CompletionResultType]::ParameterName, 'PageRank damping factor')
            [CompletionResult]::new('--max-iters', '--max-iters', [CompletionResultType]::ParameterName, 'Maximum algorithm iterations')
            [CompletionResult]::new('--tolerance', '--tolerance', [CompletionResultType]::ParameterName, 'Convergence tolerance for the algorithm')
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'Start entity IRI for shortest-path projection')
            [CompletionResult]::new('--to', '--to', [CompletionResultType]::ParameterName, 'Destination entity IRI for shortest-path projection')
            [CompletionResult]::new('--seed', '--seed', [CompletionResultType]::ParameterName, 'Seed entity for graph projection')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;report' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--type', '--type', [CompletionResultType]::ParameterName, 'Entity type IRI to select')
            [CompletionResult]::new('--predicate', '--predicate', [CompletionResultType]::ParameterName, 'Predicate IRI used to filter facts or edges')
            [CompletionResult]::new('--hubs', '--hubs', [CompletionResultType]::ParameterName, 'Number of hubs in the report')
            [CompletionResult]::new('--surprises', '--surprises', [CompletionResultType]::ParameterName, 'Number of surprising relationships in the report')
            [CompletionResult]::new('--questions', '--questions', [CompletionResultType]::ParameterName, 'Number of suggested questions in the report')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;reason' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--rules', '--rules', [CompletionResultType]::ParameterName, 'Rule set to load or evaluate')
            [CompletionResult]::new('--reactive', '--reactive', [CompletionResultType]::ParameterName, 'Use the reactive reasoner')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;demotions' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;episode' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--base-ns', '--base-ns', [CompletionResultType]::ParameterName, 'Base namespace for episode entities')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;retract' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--predicate', '--predicate', [CompletionResultType]::ParameterName, 'Predicate IRI used to filter facts or edges')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;shapes' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List the loaded SHACL shapes')
            [CompletionResult]::new('load', 'load', [CompletionResultType]::ParameterValue, 'Load a named SHACL shapes file; later writes are validated against it')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a named shapes file')
            break
        }
        'qp;shapes;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;shapes;load' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;shapes;remove' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;namespaces' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List the effective owner namespace registry')
            [CompletionResult]::new('apply', 'apply', [CompletionResultType]::ParameterValue, 'Apply the configured owner namespace registry to the store')
            break
        }
        'qp;namespaces;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;namespaces;apply' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;entity-ref' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('mint', 'mint', [CompletionResultType]::ParameterValue, 'Mint a typed stable entity reference from a JSON request')
            [CompletionResult]::new('resolve', 'resolve', [CompletionResultType]::ParameterValue, 'Resolve a stable entity reference from a JSON request')
            [CompletionResult]::new('verify', 'verify', [CompletionResultType]::ParameterValue, 'Verify a current key IRI against its local key triples')
            break
        }
        'qp;entity-ref;mint' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;entity-ref;resolve' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;entity-ref;verify' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;ontology' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List loaded OWL ontologies')
            [CompletionResult]::new('load', 'load', [CompletionResultType]::ParameterValue, 'Load an OWL ontology and materialize its entailments')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a loaded OWL ontology')
            break
        }
        'qp;ontology;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;ontology;load' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;ontology;remove' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;validate' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'Shapes used for validation or packaging')
            [CompletionResult]::new('--data', '--data', [CompletionResultType]::ParameterName, 'RDF data file to validate')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'Output or input format for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;repl' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;export' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--group-id', '--group-id', [CompletionResultType]::ParameterName, 'Select facts belonging to this group')
            [CompletionResult]::new('--construct', '--construct', [CompletionResultType]::ParameterName, 'SPARQL CONSTRUCT query selecting the share')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'Output or input format for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;render' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--dest', '--dest', [CompletionResultType]::ParameterName, 'Render output destination')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'Valid-time instant to query')
            [CompletionResult]::new('--server', '--server', [CompletionResultType]::ParameterName, 'Render service URL')
            [CompletionResult]::new('--query', '--query', [CompletionResultType]::ParameterName, 'Query used for rendering')
            [CompletionResult]::new('--template', '--template', [CompletionResultType]::ParameterName, 'Render template')
            [CompletionResult]::new('--layout', '--layout', [CompletionResultType]::ParameterName, 'Render layout')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'Relative output path template')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Preview the operation without applying it')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;stats' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;prime' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--section', '--section', [CompletionResultType]::ParameterName, 'Agent primer section to print')
            [CompletionResult]::new('--brief', '--brief', [CompletionResultType]::ParameterName, 'Print the short agent primer')
            [CompletionResult]::new('--static', '--static', [CompletionResultType]::ParameterName, 'Print the embedded primer without live context')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;search' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--mode', '--mode', [CompletionResultType]::ParameterName, 'Operation mode')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'Maximum number of results')
            [CompletionResult]::new('--alpha', '--alpha', [CompletionResultType]::ParameterName, 'Hybrid search semantic/keyword weighting')
            [CompletionResult]::new('--rrf-k', '--rrf-k', [CompletionResultType]::ParameterName, 'Reciprocal-rank fusion constant')
            [CompletionResult]::new('--decay', '--decay', [CompletionResultType]::ParameterName, 'Search recency decay factor')
            [CompletionResult]::new('--anchor', '--anchor', [CompletionResultType]::ParameterName, 'Entity IRI anchoring graph-aware search')
            [CompletionResult]::new('--anchor-mode', '--anchor-mode', [CompletionResultType]::ParameterName, 'How the anchor affects candidate selection')
            [CompletionResult]::new('--direction', '--direction', [CompletionResultType]::ParameterName, 'Traversal direction for graph-aware search')
            [CompletionResult]::new('--max-hops', '--max-hops', [CompletionResultType]::ParameterName, 'Maximum graph-search traversal hops')
            [CompletionResult]::new('--via', '--via', [CompletionResultType]::ParameterName, 'Predicate or path used for traversal')
            [CompletionResult]::new('--fusion', '--fusion', [CompletionResultType]::ParameterName, 'Search result fusion strategy')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'Valid-time instant to query')
            [CompletionResult]::new('--type', '--type', [CompletionResultType]::ParameterName, 'Entity type IRI to select')
            [CompletionResult]::new('--group', '--group', [CompletionResultType]::ParameterName, 'Group identifier for the operation')
            [CompletionResult]::new('--structured-query', '--structured-query', [CompletionResultType]::ParameterName, 'Structured search query as JSON')
            [CompletionResult]::new('--filters', '--filters', [CompletionResultType]::ParameterName, 'Structured search filters as JSON')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--graphs', '--graphs', [CompletionResultType]::ParameterName, 'Explicit graph selection for search')
            [CompletionResult]::new('--explain', '--explain', [CompletionResultType]::ParameterName, 'Include search ranking explanations')
            [CompletionResult]::new('--all-graphs', '--all-graphs', [CompletionResultType]::ParameterName, 'Search all permitted graphs explicitly')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;search-index' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'Index or graph kind to select')
            [CompletionResult]::new('--batch-size', '--batch-size', [CompletionResultType]::ParameterName, 'Number of entries per indexing batch')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Report the state of a search index')
            [CompletionResult]::new('backfill', 'backfill', [CompletionResultType]::ParameterValue, 'Backfill a search index in batches')
            [CompletionResult]::new('drop', 'drop', [CompletionResultType]::ParameterValue, 'Drop a search index')
            break
        }
        'qp;search-index;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'Index or graph kind to select')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;search-index;backfill' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'Index or graph kind to select')
            [CompletionResult]::new('--batch-size', '--batch-size', [CompletionResultType]::ParameterName, 'Number of entries per indexing batch')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;search-index;drop' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'Index or graph kind to select')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;doctor' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('labels', 'labels', [CompletionResultType]::ParameterValue, 'Report graph-label drift and recommended floors')
            break
        }
        'qp;doctor;labels' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;pack' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verify', '--verify', [CompletionResultType]::ParameterName, 'Pack file to verify without creating a new pack')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'Output file')
            [CompletionResult]::new('--space', '--space', [CompletionResultType]::ParameterName, 'Namespace space recorded in pack metadata')
            [CompletionResult]::new('--name', '--name', [CompletionResultType]::ParameterName, 'Name of the generated object')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Version recorded in pack metadata')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'Shapes used for validation or packaging')
            [CompletionResult]::new('--queries', '--queries', [CompletionResultType]::ParameterName, 'Stored queries to include')
            [CompletionResult]::new('--repo', '--repo', [CompletionResultType]::ParameterName, 'Repository identity or source root for this operation')
            [CompletionResult]::new('--repo-sha', '--repo-sha', [CompletionResultType]::ParameterName, 'Repository commit recorded in pack provenance')
            [CompletionResult]::new('--model-id', '--model-id', [CompletionResultType]::ParameterName, 'Embedding model identifier')
            [CompletionResult]::new('--model-version', '--model-version', [CompletionResultType]::ParameterName, 'Embedding model version')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'Publication destination evaluated by policy')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'Output or input format for this operation')
            [CompletionResult]::new('--with-vectors', '--with-vectors', [CompletionResultType]::ParameterName, 'Include embedding vectors in the pack')
            [CompletionResult]::new('--full', '--full', [CompletionResultType]::ParameterName, 'Include the full store or full primer, as applicable')
            [CompletionResult]::new('--allow-missing-embedding-recipe', '--allow-missing-embedding-recipe', [CompletionResultType]::ParameterName, 'Allow a pack without the embedding provider recipe')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;unpack' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--into', '--into', [CompletionResultType]::ParameterName, 'Target store for unpacking')
            [CompletionResult]::new('--expect-repo', '--expect-repo', [CompletionResultType]::ParameterName, 'Expected repository identity in the pack')
            [CompletionResult]::new('--head-sha', '--head-sha', [CompletionResultType]::ParameterName, 'Expected repository head commit')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;restore' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'Permit replacement when restoring a store')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;share' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--output', '--output', [CompletionResultType]::ParameterName, 'Output directory for the share')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--group-id', '--group-id', [CompletionResultType]::ParameterName, 'Select facts belonging to this group')
            [CompletionResult]::new('--construct', '--construct', [CompletionResultType]::ParameterName, 'SPARQL CONSTRUCT query selecting the share')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'Shapes used for validation or packaging')
            [CompletionResult]::new('--queries', '--queries', [CompletionResultType]::ParameterName, 'Stored queries to include')
            [CompletionResult]::new('--alignments', '--alignments', [CompletionResultType]::ParameterName, 'Reviewed SSSOM/RDF alignment file to carry')
            [CompletionResult]::new('--parent-share', '--parent-share', [CompletionResultType]::ParameterName, 'Parent share for a delta export')
            [CompletionResult]::new('--since', '--since', [CompletionResultType]::ParameterName, 'Starting share or transaction for incremental output')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'Publication destination evaluated by policy')
            [CompletionResult]::new('--pack-dir', '--pack-dir', [CompletionResultType]::ParameterName, 'Existing pack directory to share')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--attest-key', '--attest-key', [CompletionResultType]::ParameterName, 'Ed25519 attestation signing key file')
            [CompletionResult]::new('--attest-issued-at', '--attest-issued-at', [CompletionResultType]::ParameterName, 'Attestation issue time as Unix seconds')
            [CompletionResult]::new('--attest-ttl', '--attest-ttl', [CompletionResultType]::ParameterName, 'Attestation lifetime in seconds')
            [CompletionResult]::new('--attest-agent', '--attest-agent', [CompletionResultType]::ParameterName, 'Attested agent identifier')
            [CompletionResult]::new('--attest-session', '--attest-session', [CompletionResultType]::ParameterName, 'Attested session identifier')
            [CompletionResult]::new('--attest-introducer', '--attest-introducer', [CompletionResultType]::ParameterName, 'Attestation introducer identifier')
            [CompletionResult]::new('--attest-nonce', '--attest-nonce', [CompletionResultType]::ParameterName, 'Unique attestation nonce')
            [CompletionResult]::new('--pointers', '--pointers', [CompletionResultType]::ParameterName, 'TOML file declaring external graph pointers')
            [CompletionResult]::new('--no-shapes', '--no-shapes', [CompletionResultType]::ParameterName, 'Omit shapes from the share')
            [CompletionResult]::new('--no-queries', '--no-queries', [CompletionResultType]::ParameterName, 'Omit stored queries from the share')
            [CompletionResult]::new('--turtle', '--turtle', [CompletionResultType]::ParameterName, 'Include a human-readable Turtle export')
            [CompletionResult]::new('--attest', '--attest', [CompletionResultType]::ParameterName, 'Attach a signed agent attestation')
            [CompletionResult]::new('--project', '--project', [CompletionResultType]::ParameterName, 'Export this repository''s project graph to .quipu/graph')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('diff', 'diff', [CompletionResultType]::ParameterValue, 'Diff two packs by entity, or write a PR-review report')
            break
        }
        'qp;share;diff' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'Output or input format for this operation')
            [CompletionResult]::new('--old-shapes', '--old-shapes', [CompletionResultType]::ParameterName, 'Shapes used to validate the old share')
            [CompletionResult]::new('--new-shapes', '--new-shapes', [CompletionResultType]::ParameterName, 'Shapes used to validate the new share')
            [CompletionResult]::new('--decisions', '--decisions', [CompletionResultType]::ParameterName, 'Conflict decisions file')
            [CompletionResult]::new('--report', '--report', [CompletionResultType]::ParameterName, 'Print the validation diff report')
            [CompletionResult]::new('--fail-on-introduced', '--fail-on-introduced', [CompletionResultType]::ParameterName, 'Exit nonzero when the diff introduces violations')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;merge' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--emit-decisions', '--emit-decisions', [CompletionResultType]::ParameterName, 'Write merge decisions to this file')
            [CompletionResult]::new('--decisions', '--decisions', [CompletionResultType]::ParameterName, 'Conflict decisions file')
            [CompletionResult]::new('--reviewer', '--reviewer', [CompletionResultType]::ParameterName, 'Identity reviewing the merge')
            [CompletionResult]::new('--slot-scoped', '--slot-scoped', [CompletionResultType]::ParameterName, 'Restrict the merge to selected slots')
            [CompletionResult]::new('--propose', '--propose', [CompletionResultType]::ParameterName, 'Produce a merge proposal without applying it')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Preview the operation without applying it')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('show', 'show', [CompletionResultType]::ParameterValue, 'Show a merge''s conflicts in the standard layout')
            [CompletionResult]::new('resolve', 'resolve', [CompletionResultType]::ParameterValue, 'Record one choice on one merge conflict')
            break
        }
        'qp;merge;show' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'Output or input format for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;merge;resolve' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--choose', '--choose', [CompletionResultType]::ParameterName, 'Conflict side to choose')
            [CompletionResult]::new('--value', '--value', [CompletionResultType]::ParameterName, 'Explicit value for a conflict resolution')
            [CompletionResult]::new('--by', '--by', [CompletionResultType]::ParameterName, 'Identity recording a resolution')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;import' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--source', '--source', [CompletionResultType]::ParameterName, 'Source identifier recorded in provenance')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'Publication destination evaluated by policy')
            [CompletionResult]::new('--query-namespace', '--query-namespace', [CompletionResultType]::ParameterName, 'Namespace for imported stored queries')
            [CompletionResult]::new('--replace-queries', '--replace-queries', [CompletionResultType]::ParameterName, 'Replace imported query definitions on name conflicts')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('delta', 'delta', [CompletionResultType]::ParameterValue, 'Verify a delta share against its parent share')
            [CompletionResult]::new('promote', 'promote', [CompletionResultType]::ParameterValue, 'Promote a staged share into the store')
            [CompletionResult]::new('review', 'review', [CompletionResultType]::ParameterValue, 'Review staged shares awaiting promotion')
            break
        }
        'qp;import;delta' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'Publication destination evaluated by policy')
            [CompletionResult]::new('--query-namespace', '--query-namespace', [CompletionResultType]::ParameterName, 'Namespace for imported stored queries')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;import;promote' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;import;review' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('pending', 'pending', [CompletionResultType]::ParameterValue, 'List staged shares awaiting review')
            [CompletionResult]::new('notify', 'notify', [CompletionResultType]::ParameterValue, 'Report staged shares older than an age threshold to a route')
            [CompletionResult]::new('rejected', 'rejected', [CompletionResultType]::ParameterValue, 'Mark a staged share rejected, with a reason')
            [CompletionResult]::new('expired', 'expired', [CompletionResultType]::ParameterValue, 'Mark a staged share expired, with a reason')
            [CompletionResult]::new('reopen', 'reopen', [CompletionResultType]::ParameterValue, 'Reopen a rejected or expired staged share')
            break
        }
        'qp;import;review;pending' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--after', '--after', [CompletionResultType]::ParameterName, 'Resume after this cursor')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'Maximum number of results')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;import;review;notify' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--after', '--after', [CompletionResultType]::ParameterName, 'Resume after this cursor')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'Maximum number of results')
            [CompletionResult]::new('--age-seconds', '--age-seconds', [CompletionResultType]::ParameterName, 'Minimum notification age in seconds')
            [CompletionResult]::new('--route', '--route', [CompletionResultType]::ParameterName, 'Notification route for pending imports')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;import;review;rejected' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'Reason recorded for the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;import;review;expired' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'Reason recorded for the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;import;review;reopen' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'Reason recorded for the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;compose' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--repo', '--repo', [CompletionResultType]::ParameterName, 'Repository identity or source root for this operation')
            [CompletionResult]::new('--shapes-from', '--shapes-from', [CompletionResultType]::ParameterName, 'Pack supplying the composition shapes')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'Publication destination evaluated by policy')
            [CompletionResult]::new('--revoke-tombstone', '--revoke-tombstone', [CompletionResultType]::ParameterName, 'Tombstone triple to revoke in the composition')
            [CompletionResult]::new('--pack-key', '--pack-key', [CompletionResultType]::ParameterName, 'Composition pack key')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'Reason recorded for the operation')
            [CompletionResult]::new('--use-pointer', '--use-pointer', [CompletionResultType]::ParameterName, 'Pointer to use in the composition')
            [CompletionResult]::new('--declaration', '--declaration', [CompletionResultType]::ParameterName, 'Composition declaration file')
            [CompletionResult]::new('--authority', '--authority', [CompletionResultType]::ParameterName, 'Authority identifier for the operation')
            [CompletionResult]::new('--repo-remote', '--repo-remote', [CompletionResultType]::ParameterName, 'Repository remote used to resolve a pointer')
            [CompletionResult]::new('--pointer-cache', '--pointer-cache', [CompletionResultType]::ParameterName, 'Local directory for resolved pack pointers')
            [CompletionResult]::new('--rollback', '--rollback', [CompletionResultType]::ParameterName, 'Restore the previous CURRENT composition snapshot')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;changes' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'First transaction to include')
            [CompletionResult]::new('--capture', '--capture', [CompletionResultType]::ParameterName, 'Change-feed row capture mode')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'Maximum number of results')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;follow' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--primary', '--primary', [CompletionResultType]::ParameterName, 'Primary server URL to follow')
            [CompletionResult]::new('--page-txs', '--page-txs', [CompletionResultType]::ParameterName, 'Transactions per follower page')
            [CompletionResult]::new('--page-gap-ms', '--page-gap-ms', [CompletionResultType]::ParameterName, 'Delay between follower pages in milliseconds')
            [CompletionResult]::new('--bootstrap-only', '--bootstrap-only', [CompletionResultType]::ParameterName, 'Bootstrap a follower and exit without polling')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;acquire' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('scope', 'scope', [CompletionResultType]::ParameterValue, 'Report the bounded row and byte scope of a direct-main acquisition')
            break
        }
        'qp;acquire;scope' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--source-store-id', '--source-store-id', [CompletionResultType]::ParameterName, 'Identity of the source store being acquired')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--tx', '--tx', [CompletionResultType]::ParameterName, 'Historical transaction to read')
            [CompletionResult]::new('--max-vm-steps', '--max-vm-steps', [CompletionResultType]::ParameterName, 'Maximum SPARQL virtual-machine steps')
            [CompletionResult]::new('--deadline-ms', '--deadline-ms', [CompletionResultType]::ParameterName, 'Acquisition query deadline in milliseconds')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;migrate-vectors' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'Source vector backend')
            [CompletionResult]::new('--to', '--to', [CompletionResultType]::ParameterName, 'Destination vector backend')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Preview the operation without applying it')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;git-merge' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('show', 'show', [CompletionResultType]::ParameterValue, 'Show a qpack''s pending merge decisions in the standard layout')
            [CompletionResult]::new('resolve', 'resolve', [CompletionResultType]::ParameterValue, 'Record one choice in the Git merge in progress and rewrite the qpack')
            break
        }
        'qp;git-merge;show' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'Output or input format for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;git-merge;resolve' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--choose', '--choose', [CompletionResultType]::ParameterName, 'Conflict side to choose')
            [CompletionResult]::new('--by', '--by', [CompletionResultType]::ParameterName, 'Identity recording a resolution')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;merge-driver' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;pendant-resolve' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;pendant-check' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--history', '--history', [CompletionResultType]::ParameterName, 'Check the pendant merge history')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;diff-textconv' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;mcp' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--mcp-token-file', '--mcp-token-file', [CompletionResultType]::ParameterName, 'File containing the MCP authentication token')
            [CompletionResult]::new('--bind', '--bind', [CompletionResultType]::ParameterName, 'MCP HTTP listen address')
            [CompletionResult]::new('--primary-url', '--primary-url', [CompletionResultType]::ParameterName, 'Primary server URL used by a read-only MCP follower')
            [CompletionResult]::new('--read-only', '--read-only', [CompletionResultType]::ParameterName, 'Serve MCP in read-only mode')
            [CompletionResult]::new('--embed-backfill', '--embed-backfill', [CompletionResultType]::ParameterName, 'Enable embedding backfill in the MCP server')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;hook' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('session-capture', 'session-capture', [CompletionResultType]::ParameterValue, 'Stop hook: solicit one knowledge episode per session from stdin JSON')
            break
        }
        'qp;hook;session-capture' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;hooks' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('bundle', 'bundle', [CompletionResultType]::ParameterValue, 'Print quipu''s hook bundle (st.hook-bundle/1)')
            [CompletionResult]::new('install', 'install', [CompletionResultType]::ParameterValue, 'Install quipu''s hooks into Claude Code or Codex')
            [CompletionResult]::new('uninstall', 'uninstall', [CompletionResultType]::ParameterValue, 'Remove quipu''s hooks from Claude Code or Codex')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Report whether quipu''s hooks are installed')
            break
        }
        'qp;hooks;bundle' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;hooks;install' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--harness', '--harness', [CompletionResultType]::ParameterName, 'Agent harness: claude or codex')
            [CompletionResult]::new('--project', '--project', [CompletionResultType]::ParameterName, 'Use this repository''s project graph or project-local hooks')
            [CompletionResult]::new('--no-st', '--no-st', [CompletionResultType]::ParameterName, 'Do not integrate hooks with the crew orchestrator')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;hooks;uninstall' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--harness', '--harness', [CompletionResultType]::ParameterName, 'Agent harness: claude or codex')
            [CompletionResult]::new('--project', '--project', [CompletionResultType]::ParameterName, 'Use this repository''s project graph or project-local hooks')
            [CompletionResult]::new('--no-st', '--no-st', [CompletionResultType]::ParameterName, 'Do not integrate hooks with the crew orchestrator')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;hooks;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--harness', '--harness', [CompletionResultType]::ParameterName, 'Agent harness: claude or codex')
            [CompletionResult]::new('--project', '--project', [CompletionResultType]::ParameterName, 'Use this repository''s project graph or project-local hooks')
            [CompletionResult]::new('--no-st', '--no-st', [CompletionResultType]::ParameterName, 'Do not integrate hooks with the crew orchestrator')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;policy' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('draft', 'draft', [CompletionResultType]::ParameterValue, 'Draft advisory policy Turtle from an exemplar and an intent')
            [CompletionResult]::new('backtest', 'backtest', [CompletionResultType]::ParameterValue, 'Backtest a candidate policy over recorded transactions')
            break
        }
        'qp;policy;draft' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--exemplar', '--exemplar', [CompletionResultType]::ParameterName, 'Source exemplar for a policy draft')
            [CompletionResult]::new('--name', '--name', [CompletionResultType]::ParameterName, 'Name of the generated object')
            [CompletionResult]::new('--label', '--label', [CompletionResultType]::ParameterName, 'Human-readable label for the draft')
            [CompletionResult]::new('--targets', '--targets', [CompletionResultType]::ParameterName, 'Target class IRI for the policy')
            [CompletionResult]::new('--claim', '--claim', [CompletionResultType]::ParameterName, 'ASK query expressing the proposed policy claim')
            [CompletionResult]::new('--class', '--class', [CompletionResultType]::ParameterName, 'Policy classification: soft or hard')
            [CompletionResult]::new('--point', '--point', [CompletionResultType]::ParameterName, 'Policy verification point')
            [CompletionResult]::new('--layer', '--layer', [CompletionResultType]::ParameterName, 'Policy enforcement layer')
            [CompletionResult]::new('--authority', '--authority', [CompletionResultType]::ParameterName, 'Authority identifier for the operation')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'Output file')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;policy;backtest' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--last-txs', '--last-txs', [CompletionResultType]::ParameterName, 'Number of recent transactions to evaluate')
            [CompletionResult]::new('--from-tx', '--from-tx', [CompletionResultType]::ParameterName, 'First transaction in the selected interval')
            [CompletionResult]::new('--to-tx', '--to-tx', [CompletionResultType]::ParameterName, 'Last transaction in the selected interval')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;gate' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('shadow', 'shadow', [CompletionResultType]::ParameterValue, 'Judge a candidate policy set over recorded history on a quiescent copy')
            break
        }
        'qp;gate;shadow' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--rules', '--rules', [CompletionResultType]::ParameterName, 'Rule set to load or evaluate')
            [CompletionResult]::new('--since', '--since', [CompletionResultType]::ParameterName, 'Starting share or transaction for incremental output')
            [CompletionResult]::new('--from-tx', '--from-tx', [CompletionResultType]::ParameterName, 'First transaction in the selected interval')
            [CompletionResult]::new('--to-tx', '--to-tx', [CompletionResultType]::ParameterName, 'Last transaction in the selected interval')
            [CompletionResult]::new('--last-txs', '--last-txs', [CompletionResultType]::ParameterName, 'Number of recent transactions to evaluate')
            [CompletionResult]::new('--mode', '--mode', [CompletionResultType]::ParameterName, 'Operation mode')
            [CompletionResult]::new('--max-txs', '--max-txs', [CompletionResultType]::ParameterName, 'Maximum transactions to evaluate')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;path' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('cone', 'cone', [CompletionResultType]::ParameterValue, 'Compute the provenance cone of a trajectory')
            [CompletionResult]::new('backtest', 'backtest', [CompletionResultType]::ParameterValue, 'Backtest a pruned trajectory over recorded history')
            [CompletionResult]::new('draft', 'draft', [CompletionResultType]::ParameterValue, 'Draft a golden-path policy from a trajectory as Turtle for review')
            break
        }
        'qp;path;cone' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--via', '--via', [CompletionResultType]::ParameterName, 'Predicate or path used for traversal')
            [CompletionResult]::new('--hops', '--hops', [CompletionResultType]::ParameterName, 'Maximum traversal hops')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;path;backtest' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--omit', '--omit', [CompletionResultType]::ParameterName, 'Trajectory step to omit in a counterfactual')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;path;draft' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--name', '--name', [CompletionResultType]::ParameterName, 'Name of the generated object')
            [CompletionResult]::new('--label', '--label', [CompletionResultType]::ParameterName, 'Human-readable label for the draft')
            [CompletionResult]::new('--via', '--via', [CompletionResultType]::ParameterName, 'Predicate or path used for traversal')
            [CompletionResult]::new('--omit', '--omit', [CompletionResultType]::ParameterName, 'Trajectory step to omit in a counterfactual')
            [CompletionResult]::new('--by', '--by', [CompletionResultType]::ParameterName, 'Identity recording a resolution')
            [CompletionResult]::new('--dead-end', '--dead-end', [CompletionResultType]::ParameterName, 'Dead-end trajectory to include in the policy draft')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;align' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('propose', 'propose', [CompletionResultType]::ParameterValue, 'Propose scored alignments between two named graphs as an SSSOM set')
            [CompletionResult]::new('decide', 'decide', [CompletionResultType]::ParameterValue, 'Apply reviewer accept and negate decisions to a mapping set')
            [CompletionResult]::new('apply', 'apply', [CompletionResultType]::ParameterValue, 'Materialize decided alignments into a derived alignment graph')
            break
        }
        'qp;align;propose' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--set-id', '--set-id', [CompletionResultType]::ParameterName, 'Identifier for the proposed alignment set')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'Output file')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;align;decide' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--decisions', '--decisions', [CompletionResultType]::ParameterName, 'Conflict decisions file')
            [CompletionResult]::new('--reviewer', '--reviewer', [CompletionResultType]::ParameterName, 'Identity reviewing the merge')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'Output file')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;align;apply' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph-a', '--graph-a', [CompletionResultType]::ParameterName, 'First graph in the alignment pair')
            [CompletionResult]::new('--graph-b', '--graph-b', [CompletionResultType]::ParameterName, 'Second graph in the alignment pair')
            [CompletionResult]::new('--expected-version', '--expected-version', [CompletionResultType]::ParameterName, 'Expected version for the compare-and-swap operation')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;propose' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--status', '--status', [CompletionResultType]::ParameterName, 'Status to record or filter')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List schema change proposals, optionally by status')
            [CompletionResult]::new('submit', 'submit', [CompletionResultType]::ParameterValue, 'Submit a schema change proposal')
            [CompletionResult]::new('plan', 'plan', [CompletionResultType]::ParameterValue, 'Report a pending proposal''s blast radius and 0-100 risk score without writing; exit 0 pass, 1 over threshold, 2 not determined')
            [CompletionResult]::new('accept', 'accept', [CompletionResultType]::ParameterValue, 'Accept a pending proposal; refused when its plan is over the risk threshold unless --accept-risk --reason')
            [CompletionResult]::new('reject', 'reject', [CompletionResultType]::ParameterValue, 'Reject a pending proposal')
            break
        }
        'qp;propose;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--status', '--status', [CompletionResultType]::ParameterName, 'Status to record or filter')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;propose;submit' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--proposer', '--proposer', [CompletionResultType]::ParameterName, 'Identity proposing an alignment')
            [CompletionResult]::new('--rationale', '--rationale', [CompletionResultType]::ParameterName, 'Reason for the alignment decision')
            [CompletionResult]::new('--trigger', '--trigger', [CompletionResultType]::ParameterName, 'Trigger identifier')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;propose;plan' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'Plan output format: text or json')
            [CompletionResult]::new('--risk-threshold', '--risk-threshold', [CompletionResultType]::ParameterName, 'Fail the plan above this 0-100 risk score (default: the deployment threshold)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;propose;accept' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--decided-by', '--decided-by', [CompletionResultType]::ParameterName, 'Identity recording the alignment decision')
            [CompletionResult]::new('--note', '--note', [CompletionResultType]::ParameterName, 'Explanatory note recorded with the operation')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'Why an over-threshold plan is accepted; recorded with the decision')
            [CompletionResult]::new('--risk-threshold', '--risk-threshold', [CompletionResultType]::ParameterName, 'Refuse acceptance above this 0-100 risk score (default: the deployment threshold)')
            [CompletionResult]::new('--accept-risk', '--accept-risk', [CompletionResultType]::ParameterName, 'Accept despite a plan over the risk threshold; requires --reason')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;propose;reject' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--decided-by', '--decided-by', [CompletionResultType]::ParameterName, 'Identity recording the alignment decision')
            [CompletionResult]::new('--note', '--note', [CompletionResultType]::ParameterName, 'Explanatory note recorded with the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;memory' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('read', 'read', [CompletionResultType]::ParameterValue, 'Read a versioned memory file for an owner')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export an owner''s memory files as a file-data envelope')
            [CompletionResult]::new('receipt', 'receipt', [CompletionResultType]::ParameterValue, 'Compute the receipt a memory commit request would produce')
            [CompletionResult]::new('commit', 'commit', [CompletionResultType]::ParameterValue, 'Commit a versioned memory file change')
            break
        }
        'qp;memory;read' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;memory;export' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;memory;receipt' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;memory;commit' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--local-authority', '--local-authority', [CompletionResultType]::ParameterName, 'Explicitly authorize a local memory commit')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;attest' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('register', 'register', [CompletionResultType]::ParameterValue, 'Register a session binding for attested writes')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List registered session bindings')
            [CompletionResult]::new('allow-write', 'allow-write', [CompletionResultType]::ParameterValue, 'Allow a session to write')
            [CompletionResult]::new('deny-write', 'deny-write', [CompletionResultType]::ParameterValue, 'Stop a session from writing')
            [CompletionResult]::new('require-v2', 'require-v2', [CompletionResultType]::ParameterValue, 'Require write-v2 signatures for a session')
            [CompletionResult]::new('allow-v1', 'allow-v1', [CompletionResultType]::ParameterValue, 'Allow write-v1 signatures for a session again')
            [CompletionResult]::new('revoke', 'revoke', [CompletionResultType]::ParameterValue, 'Revoke a session binding')
            break
        }
        'qp;attest;register' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--agent', '--agent', [CompletionResultType]::ParameterName, 'Agent identifier for the binding')
            [CompletionResult]::new('--session', '--session', [CompletionResultType]::ParameterName, 'Session identifier on the statement')
            [CompletionResult]::new('--public-key', '--public-key', [CompletionResultType]::ParameterName, 'Verifier public key')
            [CompletionResult]::new('--introducer', '--introducer', [CompletionResultType]::ParameterName, 'Identifier of the binding introducer')
            [CompletionResult]::new('--issued-at', '--issued-at', [CompletionResultType]::ParameterName, 'Issue time as Unix seconds')
            [CompletionResult]::new('--expires-at', '--expires-at', [CompletionResultType]::ParameterName, 'Expiration time as Unix seconds')
            [CompletionResult]::new('--allow-write', '--allow-write', [CompletionResultType]::ParameterName, 'Grant write permission to the attested binding')
            [CompletionResult]::new('--require-v2', '--require-v2', [CompletionResultType]::ParameterName, 'Require version 2 attestations')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;attest;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;attest;allow-write' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;attest;deny-write' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;attest;require-v2' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;attest;allow-v1' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;attest;revoke' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;signer' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('enroll', 'enroll', [CompletionResultType]::ParameterValue, 'Enroll a hardware signer credential')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Show signer enrollment and enforcement state')
            [CompletionResult]::new('enable', 'enable', [CompletionResultType]::ParameterValue, 'Enable signer enforcement')
            [CompletionResult]::new('disable', 'disable', [CompletionResultType]::ParameterValue, 'Disable signer enforcement')
            [CompletionResult]::new('revoke', 'revoke', [CompletionResultType]::ParameterValue, 'Revoke a signer credential')
            [CompletionResult]::new('protect-graph', 'protect-graph', [CompletionResultType]::ParameterValue, 'Permanently require attested writes for a graph')
            break
        }
        'qp;signer;enroll' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--credential-id', '--credential-id', [CompletionResultType]::ParameterName, 'Base64url-encoded WebAuthn credential ID')
            [CompletionResult]::new('--cose-key', '--cose-key', [CompletionResultType]::ParameterName, 'Base64url-encoded COSE public key')
            [CompletionResult]::new('--scheme', '--scheme', [CompletionResultType]::ParameterName, 'Signature scheme')
            [CompletionResult]::new('--origin', '--origin', [CompletionResultType]::ParameterName, 'WebAuthn origin URL')
            [CompletionResult]::new('--rp-id', '--rp-id', [CompletionResultType]::ParameterName, 'WebAuthn relying-party ID')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--expires-at', '--expires-at', [CompletionResultType]::ParameterName, 'Expiration time as Unix seconds')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;signer;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;signer;enable' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;signer;disable' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;signer;revoke' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;signer;protect-graph' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;deck' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('configure', 'configure', [CompletionResultType]::ParameterValue, 'Configure the Decision Deck scope (starts disabled)')
            [CompletionResult]::new('enroll', 'enroll', [CompletionResultType]::ParameterValue, 'Enroll a Decision Deck credential')
            [CompletionResult]::new('enable', 'enable', [CompletionResultType]::ParameterValue, 'Enable the Decision Deck')
            [CompletionResult]::new('disable', 'disable', [CompletionResultType]::ParameterValue, 'Disable the Decision Deck')
            [CompletionResult]::new('revoke', 'revoke', [CompletionResultType]::ParameterValue, 'Revoke a Decision Deck credential')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Show Decision Deck configuration and state')
            break
        }
        'qp;deck;configure' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--origin', '--origin', [CompletionResultType]::ParameterName, 'WebAuthn origin URL')
            [CompletionResult]::new('--rp-id', '--rp-id', [CompletionResultType]::ParameterName, 'WebAuthn relying-party ID')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--author', '--author', [CompletionResultType]::ParameterName, 'Author identifier on the signed statement')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;deck;enroll' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--credential-id', '--credential-id', [CompletionResultType]::ParameterName, 'Base64url-encoded WebAuthn credential ID')
            [CompletionResult]::new('--cose-key', '--cose-key', [CompletionResultType]::ParameterName, 'Base64url-encoded COSE public key')
            [CompletionResult]::new('--scheme', '--scheme', [CompletionResultType]::ParameterName, 'Signature scheme')
            [CompletionResult]::new('--expires-at', '--expires-at', [CompletionResultType]::ParameterName, 'Expiration time as Unix seconds')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;deck;enable' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;deck;disable' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;deck;revoke' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;deck;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;trust-root' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('challenge', 'challenge', [CompletionResultType]::ParameterValue, 'Issue the proof-of-possession challenge for a first verifier key')
            [CompletionResult]::new('bootstrap', 'bootstrap', [CompletionResultType]::ParameterValue, 'Enroll the first verifier key with its signed challenge')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Show the root-of-trust state')
            break
        }
        'qp;trust-root;challenge' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verifier', '--verifier', [CompletionResultType]::ParameterName, 'Verifier identifier for a signed statement')
            [CompletionResult]::new('--public-key', '--public-key', [CompletionResultType]::ParameterName, 'Verifier public key')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;trust-root;bootstrap' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verifier', '--verifier', [CompletionResultType]::ParameterName, 'Verifier identifier for a signed statement')
            [CompletionResult]::new('--public-key', '--public-key', [CompletionResultType]::ParameterName, 'Verifier public key')
            [CompletionResult]::new('--pop-signature', '--pop-signature', [CompletionResultType]::ParameterName, 'Proof-of-possession signature')
            [CompletionResult]::new('--attests', '--attests', [CompletionResultType]::ParameterName, 'IRI of the object being attested')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;trust-root;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('create', 'create', [CompletionResultType]::ParameterValue, 'Register a committed named graph')
            [CompletionResult]::new('label', 'label', [CompletionResultType]::ParameterValue, 'Declare lattice labels on a registered graph')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List registered named graphs')
            [CompletionResult]::new('import', 'import', [CompletionResultType]::ParameterValue, 'Import another store file as a named graph')
            [CompletionResult]::new('freeze', 'freeze', [CompletionResultType]::ParameterValue, 'Freeze a graph''s full history into a read-only archive pack')
            [CompletionResult]::new('thaw', 'thaw', [CompletionResultType]::ParameterValue, 'Restore a frozen graph from its archive pack')
            [CompletionResult]::new('overlay-create', 'overlay-create', [CompletionResultType]::ParameterValue, 'Create an overlay graph bound to a committed parent')
            [CompletionResult]::new('overlay-write', 'overlay-write', [CompletionResultType]::ParameterValue, 'Assert, retract or tombstone a triple in an overlay')
            [CompletionResult]::new('overlay-compose', 'overlay-compose', [CompletionResultType]::ParameterValue, 'Resolve an overlay''s composed view over its parent')
            break
        }
        'qp;graph;create' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph;label' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--label', '--label', [CompletionResultType]::ParameterName, 'Human-readable label for the draft')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'Index or graph kind to select')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--frozen', '--frozen', [CompletionResultType]::ParameterName, 'List only frozen graphs')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph;import' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--as', '--as', [CompletionResultType]::ParameterName, 'IRI under which to import the graph')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph;freeze' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'Output file')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph;thaw' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph;overlay-create' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--input', '--input', [CompletionResultType]::ParameterName, 'Input file for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph;overlay-write' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--input', '--input', [CompletionResultType]::ParameterName, 'Input file for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;graph;overlay-compose' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--input', '--input', [CompletionResultType]::ParameterName, 'Input file for this operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;db' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('respace', 'respace', [CompletionResultType]::ParameterValue, 'Copy the store into a term space')
            [CompletionResult]::new('attach', 'attach', [CompletionResultType]::ParameterValue, 'List the databases mounted alongside this store')
            [CompletionResult]::new('migrate-inferred', 'migrate-inferred', [CompletionResultType]::ParameterValue, 'Move engine-derived facts into their companion inferred graphs')
            break
        }
        'qp;db;respace' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--into', '--into', [CompletionResultType]::ParameterName, 'New integer namespace space number')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'Output file')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;db;attach' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--list', '--list', [CompletionResultType]::ParameterName, 'List mounted read-only databases')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;db;migrate-inferred' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;events' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('refusals', 'refusals', [CompletionResultType]::ParameterValue, 'List recorded write refusals')
            break
        }
        'qp;events;refusals' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;fork' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--name', '--name', [CompletionResultType]::ParameterName, 'Name of the generated object')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List named forks')
            [CompletionResult]::new('diff', 'diff', [CompletionResultType]::ParameterValue, 'Diff two forks')
            [CompletionResult]::new('drop', 'drop', [CompletionResultType]::ParameterValue, 'Drop a named fork')
            [CompletionResult]::new('promote', 'promote', [CompletionResultType]::ParameterValue, 'Promote a fork to ROOT, with reviewable conflict decisions')
            break
        }
        'qp;fork;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;fork;diff' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;fork;drop' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;fork;promote' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'Valid-time timestamp for the operation')
            [CompletionResult]::new('--emit-decisions', '--emit-decisions', [CompletionResultType]::ParameterName, 'Write merge decisions to this file')
            [CompletionResult]::new('--decisions', '--decisions', [CompletionResultType]::ParameterName, 'Conflict decisions file')
            [CompletionResult]::new('--reviewer', '--reviewer', [CompletionResultType]::ParameterName, 'Identity reviewing the merge')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'Actor recorded in operation provenance')
            [CompletionResult]::new('--propose', '--propose', [CompletionResultType]::ParameterName, 'Produce a merge proposal without applying it')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Preview the operation without applying it')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;audit' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--repo', '--repo', [CompletionResultType]::ParameterName, 'Repository identity or source root for this operation')
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'Base Git revision for the audit')
            [CompletionResult]::new('--to', '--to', [CompletionResultType]::ParameterName, 'Tip Git revision for the audit')
            [CompletionResult]::new('--yupana', '--yupana', [CompletionResultType]::ParameterName, 'Yupana executable used for the audit')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('inventory', 'inventory', [CompletionResultType]::ParameterValue, 'List the policies the audit checks against')
            [CompletionResult]::new('namespace', 'namespace', [CompletionResultType]::ParameterValue, 'Report base-namespace drift')
            [CompletionResult]::new('replay', 'replay', [CompletionResultType]::ParameterValue, 'Re-derive a gate decision as of its transaction')
            [CompletionResult]::new('tree', 'tree', [CompletionResultType]::ParameterValue, 'Reassemble the dispatch forest from a trace''s principal chains')
            [CompletionResult]::new('inheritance', 'inheritance', [CompletionResultType]::ParameterValue, 'Check whether inherited constraints stopped applying under delegation')
            [CompletionResult]::new('quarantine', 'quarantine', [CompletionResultType]::ParameterValue, 'List refused-attempt evidence behind verdicts')
            break
        }
        'qp;audit;inventory' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;audit;namespace' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;audit;replay' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--delta', '--delta', [CompletionResultType]::ParameterName, 'Refused-delta file for verdict replay')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;audit;tree' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;audit;inheritance' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;audit;quarantine' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verdict', '--verdict', [CompletionResultType]::ParameterName, 'Verdict identifier to inspect')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List refused-attempt evidence, optionally for one verdict')
            [CompletionResult]::new('purge', 'purge', [CompletionResultType]::ParameterValue, 'Purge quarantined evidence by graph, age or all')
            break
        }
        'qp;audit;quarantine;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verdict', '--verdict', [CompletionResultType]::ParameterName, 'Verdict identifier to inspect')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'qp;audit;quarantine;purge' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'Explicit graph IRI for this operation')
            [CompletionResult]::new('--before', '--before', [CompletionResultType]::ParameterName, 'Purge entries older than this timestamp')
            [CompletionResult]::new('--older-than', '--older-than', [CompletionResultType]::ParameterName, 'Minimum age for quarantine purging')
            [CompletionResult]::new('--all', '--all', [CompletionResultType]::ParameterName, 'Select all retained quarantine entries')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Print machine-readable JSON')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
