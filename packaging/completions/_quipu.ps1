
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'quipu' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'quipu'
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
        'quipu' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
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
            [CompletionResult]::new('propose', 'propose', [CompletionResultType]::ParameterValue, 'List, submit, accept or reject schema change proposals')
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
        'quipu;version' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;install-alias' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--bin-dir', '--bin-dir', [CompletionResultType]::ParameterName, 'bin-dir')
            [CompletionResult]::new('--check', '--check', [CompletionResultType]::ParameterName, 'check')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;knot' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'shapes')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'timestamp')
            [CompletionResult]::new('--blank-node-scope', '--blank-node-scope', [CompletionResultType]::ParameterName, 'blank-node-scope')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;load' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'shapes')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'timestamp')
            [CompletionResult]::new('--blank-node-scope', '--blank-node-scope', [CompletionResultType]::ParameterName, 'blank-node-scope')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'destination')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;ingest' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'timestamp')
            [CompletionResult]::new('--declare-count', '--declare-count', [CompletionResultType]::ParameterName, 'declare-count')
            [CompletionResult]::new('--declare-sha256', '--declare-sha256', [CompletionResultType]::ParameterName, 'declare-sha256')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'format')
            [CompletionResult]::new('--chunk', '--chunk', [CompletionResultType]::ParameterName, 'chunk')
            [CompletionResult]::new('--base-iri', '--base-iri', [CompletionResultType]::ParameterName, 'base-iri')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--source', '--source', [CompletionResultType]::ParameterName, 'source')
            [CompletionResult]::new('--blank-node-scope', '--blank-node-scope', [CompletionResultType]::ParameterName, 'blank-node-scope')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;read' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'valid-at')
            [CompletionResult]::new('--tx', '--tx', [CompletionResultType]::ParameterName, 'tx')
            [CompletionResult]::new('--fork', '--fork', [CompletionResultType]::ParameterName, 'fork')
            [CompletionResult]::new('--entailment', '--entailment', [CompletionResultType]::ParameterName, 'entailment')
            [CompletionResult]::new('--federation', '--federation', [CompletionResultType]::ParameterName, 'federation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;query' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'valid-at')
            [CompletionResult]::new('--tx', '--tx', [CompletionResultType]::ParameterName, 'tx')
            [CompletionResult]::new('--fork', '--fork', [CompletionResultType]::ParameterName, 'fork')
            [CompletionResult]::new('--entailment', '--entailment', [CompletionResultType]::ParameterName, 'entailment')
            [CompletionResult]::new('--federation', '--federation', [CompletionResultType]::ParameterName, 'federation')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;rule-query' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;cord' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--type', '--type', [CompletionResultType]::ParameterName, 'type')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'limit')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;unravel' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--tx', '--tx', [CompletionResultType]::ParameterName, 'tx')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'valid-at')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;impact' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--hops', '--hops', [CompletionResultType]::ParameterName, 'hops')
            [CompletionResult]::new('--predicate', '--predicate', [CompletionResultType]::ParameterName, 'predicate')
            [CompletionResult]::new('--remove', '--remove', [CompletionResultType]::ParameterName, 'remove')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;explain' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--depth', '--depth', [CompletionResultType]::ParameterName, 'depth')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;project' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--algorithm', '--algorithm', [CompletionResultType]::ParameterName, 'algorithm')
            [CompletionResult]::new('--type', '--type', [CompletionResultType]::ParameterName, 'type')
            [CompletionResult]::new('--predicate', '--predicate', [CompletionResultType]::ParameterName, 'predicate')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'limit')
            [CompletionResult]::new('--damping', '--damping', [CompletionResultType]::ParameterName, 'damping')
            [CompletionResult]::new('--max-iters', '--max-iters', [CompletionResultType]::ParameterName, 'max-iters')
            [CompletionResult]::new('--tolerance', '--tolerance', [CompletionResultType]::ParameterName, 'tolerance')
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'from')
            [CompletionResult]::new('--to', '--to', [CompletionResultType]::ParameterName, 'to')
            [CompletionResult]::new('--seed', '--seed', [CompletionResultType]::ParameterName, 'seed')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;report' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--type', '--type', [CompletionResultType]::ParameterName, 'type')
            [CompletionResult]::new('--predicate', '--predicate', [CompletionResultType]::ParameterName, 'predicate')
            [CompletionResult]::new('--hubs', '--hubs', [CompletionResultType]::ParameterName, 'hubs')
            [CompletionResult]::new('--surprises', '--surprises', [CompletionResultType]::ParameterName, 'surprises')
            [CompletionResult]::new('--questions', '--questions', [CompletionResultType]::ParameterName, 'questions')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;reason' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--rules', '--rules', [CompletionResultType]::ParameterName, 'rules')
            [CompletionResult]::new('--reactive', '--reactive', [CompletionResultType]::ParameterName, 'reactive')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;demotions' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;episode' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--base-ns', '--base-ns', [CompletionResultType]::ParameterName, 'base-ns')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'timestamp')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;retract' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--predicate', '--predicate', [CompletionResultType]::ParameterName, 'predicate')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'timestamp')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;shapes' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List the loaded SHACL shapes')
            [CompletionResult]::new('load', 'load', [CompletionResultType]::ParameterValue, 'Load a named SHACL shapes file; later writes are validated against it')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a named shapes file')
            break
        }
        'quipu;shapes;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;shapes;load' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;shapes;remove' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;namespaces' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List the effective owner namespace registry')
            [CompletionResult]::new('apply', 'apply', [CompletionResultType]::ParameterValue, 'Apply the configured owner namespace registry to the store')
            break
        }
        'quipu;namespaces;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;namespaces;apply' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;entity-ref' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('mint', 'mint', [CompletionResultType]::ParameterValue, 'Mint a typed stable entity reference from a JSON request')
            [CompletionResult]::new('resolve', 'resolve', [CompletionResultType]::ParameterValue, 'Resolve a stable entity reference from a JSON request')
            break
        }
        'quipu;entity-ref;mint' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;entity-ref;resolve' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;ontology' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List loaded OWL ontologies')
            [CompletionResult]::new('load', 'load', [CompletionResultType]::ParameterValue, 'Load an OWL ontology and materialize its entailments')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a loaded OWL ontology')
            break
        }
        'quipu;ontology;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;ontology;load' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;ontology;remove' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;validate' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'shapes')
            [CompletionResult]::new('--data', '--data', [CompletionResultType]::ParameterName, 'data')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'format')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;repl' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;export' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--group-id', '--group-id', [CompletionResultType]::ParameterName, 'group-id')
            [CompletionResult]::new('--construct', '--construct', [CompletionResultType]::ParameterName, 'construct')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'format')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;render' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--dest', '--dest', [CompletionResultType]::ParameterName, 'dest')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'valid-at')
            [CompletionResult]::new('--server', '--server', [CompletionResultType]::ParameterName, 'server')
            [CompletionResult]::new('--query', '--query', [CompletionResultType]::ParameterName, 'query')
            [CompletionResult]::new('--template', '--template', [CompletionResultType]::ParameterName, 'template')
            [CompletionResult]::new('--layout', '--layout', [CompletionResultType]::ParameterName, 'layout')
            [CompletionResult]::new('--path', '--path', [CompletionResultType]::ParameterName, 'path')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'dry-run')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;stats' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;prime' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--section', '--section', [CompletionResultType]::ParameterName, 'section')
            [CompletionResult]::new('--brief', '--brief', [CompletionResultType]::ParameterName, 'brief')
            [CompletionResult]::new('--static', '--static', [CompletionResultType]::ParameterName, 'static')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;search' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--mode', '--mode', [CompletionResultType]::ParameterName, 'mode')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'limit')
            [CompletionResult]::new('--alpha', '--alpha', [CompletionResultType]::ParameterName, 'alpha')
            [CompletionResult]::new('--rrf-k', '--rrf-k', [CompletionResultType]::ParameterName, 'rrf-k')
            [CompletionResult]::new('--decay', '--decay', [CompletionResultType]::ParameterName, 'decay')
            [CompletionResult]::new('--anchor', '--anchor', [CompletionResultType]::ParameterName, 'anchor')
            [CompletionResult]::new('--anchor-mode', '--anchor-mode', [CompletionResultType]::ParameterName, 'anchor-mode')
            [CompletionResult]::new('--direction', '--direction', [CompletionResultType]::ParameterName, 'direction')
            [CompletionResult]::new('--max-hops', '--max-hops', [CompletionResultType]::ParameterName, 'max-hops')
            [CompletionResult]::new('--via', '--via', [CompletionResultType]::ParameterName, 'via')
            [CompletionResult]::new('--fusion', '--fusion', [CompletionResultType]::ParameterName, 'fusion')
            [CompletionResult]::new('--valid-at', '--valid-at', [CompletionResultType]::ParameterName, 'valid-at')
            [CompletionResult]::new('--type', '--type', [CompletionResultType]::ParameterName, 'type')
            [CompletionResult]::new('--group', '--group', [CompletionResultType]::ParameterName, 'group')
            [CompletionResult]::new('--structured-query', '--structured-query', [CompletionResultType]::ParameterName, 'structured-query')
            [CompletionResult]::new('--filters', '--filters', [CompletionResultType]::ParameterName, 'filters')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--graphs', '--graphs', [CompletionResultType]::ParameterName, 'graphs')
            [CompletionResult]::new('--explain', '--explain', [CompletionResultType]::ParameterName, 'explain')
            [CompletionResult]::new('--all-graphs', '--all-graphs', [CompletionResultType]::ParameterName, 'all-graphs')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;search-index' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'kind')
            [CompletionResult]::new('--batch-size', '--batch-size', [CompletionResultType]::ParameterName, 'batch-size')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Report the state of a search index')
            [CompletionResult]::new('backfill', 'backfill', [CompletionResultType]::ParameterValue, 'Backfill a search index in batches')
            [CompletionResult]::new('drop', 'drop', [CompletionResultType]::ParameterValue, 'Drop a search index')
            break
        }
        'quipu;search-index;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'kind')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;search-index;backfill' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'kind')
            [CompletionResult]::new('--batch-size', '--batch-size', [CompletionResultType]::ParameterName, 'batch-size')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;search-index;drop' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'kind')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;doctor' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('labels', 'labels', [CompletionResultType]::ParameterValue, 'Report graph-label drift and recommended floors')
            break
        }
        'quipu;doctor;labels' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;pack' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verify', '--verify', [CompletionResultType]::ParameterName, 'verify')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'out')
            [CompletionResult]::new('--space', '--space', [CompletionResultType]::ParameterName, 'space')
            [CompletionResult]::new('--name', '--name', [CompletionResultType]::ParameterName, 'name')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'version')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'shapes')
            [CompletionResult]::new('--queries', '--queries', [CompletionResultType]::ParameterName, 'queries')
            [CompletionResult]::new('--repo', '--repo', [CompletionResultType]::ParameterName, 'repo')
            [CompletionResult]::new('--repo-sha', '--repo-sha', [CompletionResultType]::ParameterName, 'repo-sha')
            [CompletionResult]::new('--model-id', '--model-id', [CompletionResultType]::ParameterName, 'model-id')
            [CompletionResult]::new('--model-version', '--model-version', [CompletionResultType]::ParameterName, 'model-version')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'destination')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'format')
            [CompletionResult]::new('--with-vectors', '--with-vectors', [CompletionResultType]::ParameterName, 'with-vectors')
            [CompletionResult]::new('--full', '--full', [CompletionResultType]::ParameterName, 'full')
            [CompletionResult]::new('--allow-missing-embedding-recipe', '--allow-missing-embedding-recipe', [CompletionResultType]::ParameterName, 'allow-missing-embedding-recipe')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;unpack' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--into', '--into', [CompletionResultType]::ParameterName, 'into')
            [CompletionResult]::new('--expect-repo', '--expect-repo', [CompletionResultType]::ParameterName, 'expect-repo')
            [CompletionResult]::new('--head-sha', '--head-sha', [CompletionResultType]::ParameterName, 'head-sha')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;restore' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'force')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;share' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--output', '--output', [CompletionResultType]::ParameterName, 'output')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--group-id', '--group-id', [CompletionResultType]::ParameterName, 'group-id')
            [CompletionResult]::new('--construct', '--construct', [CompletionResultType]::ParameterName, 'construct')
            [CompletionResult]::new('--shapes', '--shapes', [CompletionResultType]::ParameterName, 'shapes')
            [CompletionResult]::new('--queries', '--queries', [CompletionResultType]::ParameterName, 'queries')
            [CompletionResult]::new('--alignments', '--alignments', [CompletionResultType]::ParameterName, 'alignments')
            [CompletionResult]::new('--parent-share', '--parent-share', [CompletionResultType]::ParameterName, 'parent-share')
            [CompletionResult]::new('--since', '--since', [CompletionResultType]::ParameterName, 'since')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'destination')
            [CompletionResult]::new('--pack-dir', '--pack-dir', [CompletionResultType]::ParameterName, 'pack-dir')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--attest-key', '--attest-key', [CompletionResultType]::ParameterName, 'attest-key')
            [CompletionResult]::new('--attest-issued-at', '--attest-issued-at', [CompletionResultType]::ParameterName, 'attest-issued-at')
            [CompletionResult]::new('--attest-ttl', '--attest-ttl', [CompletionResultType]::ParameterName, 'attest-ttl')
            [CompletionResult]::new('--attest-agent', '--attest-agent', [CompletionResultType]::ParameterName, 'attest-agent')
            [CompletionResult]::new('--attest-session', '--attest-session', [CompletionResultType]::ParameterName, 'attest-session')
            [CompletionResult]::new('--attest-introducer', '--attest-introducer', [CompletionResultType]::ParameterName, 'attest-introducer')
            [CompletionResult]::new('--attest-nonce', '--attest-nonce', [CompletionResultType]::ParameterName, 'attest-nonce')
            [CompletionResult]::new('--pointers', '--pointers', [CompletionResultType]::ParameterName, 'pointers')
            [CompletionResult]::new('--no-shapes', '--no-shapes', [CompletionResultType]::ParameterName, 'no-shapes')
            [CompletionResult]::new('--no-queries', '--no-queries', [CompletionResultType]::ParameterName, 'no-queries')
            [CompletionResult]::new('--turtle', '--turtle', [CompletionResultType]::ParameterName, 'turtle')
            [CompletionResult]::new('--attest', '--attest', [CompletionResultType]::ParameterName, 'attest')
            [CompletionResult]::new('--project', '--project', [CompletionResultType]::ParameterName, 'project')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('diff', 'diff', [CompletionResultType]::ParameterValue, 'Diff two packs by entity, or write a PR-review report')
            break
        }
        'quipu;share;diff' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--format', '--format', [CompletionResultType]::ParameterName, 'format')
            [CompletionResult]::new('--old-shapes', '--old-shapes', [CompletionResultType]::ParameterName, 'old-shapes')
            [CompletionResult]::new('--new-shapes', '--new-shapes', [CompletionResultType]::ParameterName, 'new-shapes')
            [CompletionResult]::new('--decisions', '--decisions', [CompletionResultType]::ParameterName, 'decisions')
            [CompletionResult]::new('--report', '--report', [CompletionResultType]::ParameterName, 'report')
            [CompletionResult]::new('--fail-on-introduced', '--fail-on-introduced', [CompletionResultType]::ParameterName, 'fail-on-introduced')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;merge' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--emit-decisions', '--emit-decisions', [CompletionResultType]::ParameterName, 'emit-decisions')
            [CompletionResult]::new('--decisions', '--decisions', [CompletionResultType]::ParameterName, 'decisions')
            [CompletionResult]::new('--reviewer', '--reviewer', [CompletionResultType]::ParameterName, 'reviewer')
            [CompletionResult]::new('--slot-scoped', '--slot-scoped', [CompletionResultType]::ParameterName, 'slot-scoped')
            [CompletionResult]::new('--propose', '--propose', [CompletionResultType]::ParameterName, 'propose')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'dry-run')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;import' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--source', '--source', [CompletionResultType]::ParameterName, 'source')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'destination')
            [CompletionResult]::new('--query-namespace', '--query-namespace', [CompletionResultType]::ParameterName, 'query-namespace')
            [CompletionResult]::new('--replace-queries', '--replace-queries', [CompletionResultType]::ParameterName, 'replace-queries')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('delta', 'delta', [CompletionResultType]::ParameterValue, 'Verify a delta share against its parent share')
            [CompletionResult]::new('promote', 'promote', [CompletionResultType]::ParameterValue, 'Promote a staged share into the store')
            [CompletionResult]::new('review', 'review', [CompletionResultType]::ParameterValue, 'Review staged shares awaiting promotion')
            break
        }
        'quipu;import;delta' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'destination')
            [CompletionResult]::new('--query-namespace', '--query-namespace', [CompletionResultType]::ParameterName, 'query-namespace')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;import;promote' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;import;review' {
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
        'quipu;import;review;pending' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--after', '--after', [CompletionResultType]::ParameterName, 'after')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'limit')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;import;review;notify' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--after', '--after', [CompletionResultType]::ParameterName, 'after')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'limit')
            [CompletionResult]::new('--age-seconds', '--age-seconds', [CompletionResultType]::ParameterName, 'age-seconds')
            [CompletionResult]::new('--route', '--route', [CompletionResultType]::ParameterName, 'route')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;import;review;rejected' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'reason')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;import;review;expired' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'reason')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;import;review;reopen' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'reason')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;compose' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--repo', '--repo', [CompletionResultType]::ParameterName, 'repo')
            [CompletionResult]::new('--shapes-from', '--shapes-from', [CompletionResultType]::ParameterName, 'shapes-from')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--destination', '--destination', [CompletionResultType]::ParameterName, 'destination')
            [CompletionResult]::new('--revoke-tombstone', '--revoke-tombstone', [CompletionResultType]::ParameterName, 'revoke-tombstone')
            [CompletionResult]::new('--pack-key', '--pack-key', [CompletionResultType]::ParameterName, 'pack-key')
            [CompletionResult]::new('--reason', '--reason', [CompletionResultType]::ParameterName, 'reason')
            [CompletionResult]::new('--use-pointer', '--use-pointer', [CompletionResultType]::ParameterName, 'use-pointer')
            [CompletionResult]::new('--declaration', '--declaration', [CompletionResultType]::ParameterName, 'declaration')
            [CompletionResult]::new('--authority', '--authority', [CompletionResultType]::ParameterName, 'authority')
            [CompletionResult]::new('--repo-remote', '--repo-remote', [CompletionResultType]::ParameterName, 'repo-remote')
            [CompletionResult]::new('--pointer-cache', '--pointer-cache', [CompletionResultType]::ParameterName, 'pointer-cache')
            [CompletionResult]::new('--rollback', '--rollback', [CompletionResultType]::ParameterName, 'rollback')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;changes' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'from')
            [CompletionResult]::new('--capture', '--capture', [CompletionResultType]::ParameterName, 'capture')
            [CompletionResult]::new('--limit', '--limit', [CompletionResultType]::ParameterName, 'limit')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;follow' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--primary', '--primary', [CompletionResultType]::ParameterName, 'primary')
            [CompletionResult]::new('--page-txs', '--page-txs', [CompletionResultType]::ParameterName, 'page-txs')
            [CompletionResult]::new('--page-gap-ms', '--page-gap-ms', [CompletionResultType]::ParameterName, 'page-gap-ms')
            [CompletionResult]::new('--bootstrap-only', '--bootstrap-only', [CompletionResultType]::ParameterName, 'bootstrap-only')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;acquire' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('scope', 'scope', [CompletionResultType]::ParameterValue, 'Report the bounded row and byte scope of a direct-main acquisition')
            break
        }
        'quipu;acquire;scope' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--source-store-id', '--source-store-id', [CompletionResultType]::ParameterName, 'source-store-id')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--tx', '--tx', [CompletionResultType]::ParameterName, 'tx')
            [CompletionResult]::new('--max-vm-steps', '--max-vm-steps', [CompletionResultType]::ParameterName, 'max-vm-steps')
            [CompletionResult]::new('--deadline-ms', '--deadline-ms', [CompletionResultType]::ParameterName, 'deadline-ms')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;migrate-vectors' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'from')
            [CompletionResult]::new('--to', '--to', [CompletionResultType]::ParameterName, 'to')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'dry-run')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;git-merge' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;merge-driver' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;pendant-resolve' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;pendant-check' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--history', '--history', [CompletionResultType]::ParameterName, 'history')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;diff-textconv' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;mcp' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--mcp-token-file', '--mcp-token-file', [CompletionResultType]::ParameterName, 'mcp-token-file')
            [CompletionResult]::new('--bind', '--bind', [CompletionResultType]::ParameterName, 'bind')
            [CompletionResult]::new('--primary-url', '--primary-url', [CompletionResultType]::ParameterName, 'primary-url')
            [CompletionResult]::new('--read-only', '--read-only', [CompletionResultType]::ParameterName, 'read-only')
            [CompletionResult]::new('--embed-backfill', '--embed-backfill', [CompletionResultType]::ParameterName, 'embed-backfill')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;hook' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('session-capture', 'session-capture', [CompletionResultType]::ParameterValue, 'Stop hook: solicit one knowledge episode per session from stdin JSON')
            break
        }
        'quipu;hook;session-capture' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;hooks' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('bundle', 'bundle', [CompletionResultType]::ParameterValue, 'Print quipu''s hook bundle (st.hook-bundle/1)')
            [CompletionResult]::new('install', 'install', [CompletionResultType]::ParameterValue, 'Install quipu''s hooks into Claude Code or Codex')
            [CompletionResult]::new('uninstall', 'uninstall', [CompletionResultType]::ParameterValue, 'Remove quipu''s hooks from Claude Code or Codex')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Report whether quipu''s hooks are installed')
            break
        }
        'quipu;hooks;bundle' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;hooks;install' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--harness', '--harness', [CompletionResultType]::ParameterName, 'harness')
            [CompletionResult]::new('--project', '--project', [CompletionResultType]::ParameterName, 'project')
            [CompletionResult]::new('--no-st', '--no-st', [CompletionResultType]::ParameterName, 'no-st')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;hooks;uninstall' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--harness', '--harness', [CompletionResultType]::ParameterName, 'harness')
            [CompletionResult]::new('--project', '--project', [CompletionResultType]::ParameterName, 'project')
            [CompletionResult]::new('--no-st', '--no-st', [CompletionResultType]::ParameterName, 'no-st')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;hooks;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--harness', '--harness', [CompletionResultType]::ParameterName, 'harness')
            [CompletionResult]::new('--project', '--project', [CompletionResultType]::ParameterName, 'project')
            [CompletionResult]::new('--no-st', '--no-st', [CompletionResultType]::ParameterName, 'no-st')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;policy' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('draft', 'draft', [CompletionResultType]::ParameterValue, 'Draft advisory policy Turtle from an exemplar and an intent')
            [CompletionResult]::new('backtest', 'backtest', [CompletionResultType]::ParameterValue, 'Backtest a candidate policy over recorded transactions')
            break
        }
        'quipu;policy;draft' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--exemplar', '--exemplar', [CompletionResultType]::ParameterName, 'exemplar')
            [CompletionResult]::new('--name', '--name', [CompletionResultType]::ParameterName, 'name')
            [CompletionResult]::new('--label', '--label', [CompletionResultType]::ParameterName, 'label')
            [CompletionResult]::new('--targets', '--targets', [CompletionResultType]::ParameterName, 'targets')
            [CompletionResult]::new('--claim', '--claim', [CompletionResultType]::ParameterName, 'claim')
            [CompletionResult]::new('--class', '--class', [CompletionResultType]::ParameterName, 'class')
            [CompletionResult]::new('--point', '--point', [CompletionResultType]::ParameterName, 'point')
            [CompletionResult]::new('--layer', '--layer', [CompletionResultType]::ParameterName, 'layer')
            [CompletionResult]::new('--authority', '--authority', [CompletionResultType]::ParameterName, 'authority')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'out')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;policy;backtest' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--last-txs', '--last-txs', [CompletionResultType]::ParameterName, 'last-txs')
            [CompletionResult]::new('--from-tx', '--from-tx', [CompletionResultType]::ParameterName, 'from-tx')
            [CompletionResult]::new('--to-tx', '--to-tx', [CompletionResultType]::ParameterName, 'to-tx')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;gate' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('shadow', 'shadow', [CompletionResultType]::ParameterValue, 'Judge a candidate policy set over recorded history on a quiescent copy')
            break
        }
        'quipu;gate;shadow' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--rules', '--rules', [CompletionResultType]::ParameterName, 'rules')
            [CompletionResult]::new('--since', '--since', [CompletionResultType]::ParameterName, 'since')
            [CompletionResult]::new('--from-tx', '--from-tx', [CompletionResultType]::ParameterName, 'from-tx')
            [CompletionResult]::new('--to-tx', '--to-tx', [CompletionResultType]::ParameterName, 'to-tx')
            [CompletionResult]::new('--last-txs', '--last-txs', [CompletionResultType]::ParameterName, 'last-txs')
            [CompletionResult]::new('--mode', '--mode', [CompletionResultType]::ParameterName, 'mode')
            [CompletionResult]::new('--max-txs', '--max-txs', [CompletionResultType]::ParameterName, 'max-txs')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;path' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('cone', 'cone', [CompletionResultType]::ParameterValue, 'Compute the provenance cone of a trajectory')
            [CompletionResult]::new('backtest', 'backtest', [CompletionResultType]::ParameterValue, 'Backtest a pruned trajectory over recorded history')
            [CompletionResult]::new('draft', 'draft', [CompletionResultType]::ParameterValue, 'Draft a golden-path policy from a trajectory as Turtle for review')
            break
        }
        'quipu;path;cone' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--via', '--via', [CompletionResultType]::ParameterName, 'via')
            [CompletionResult]::new('--hops', '--hops', [CompletionResultType]::ParameterName, 'hops')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;path;backtest' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--omit', '--omit', [CompletionResultType]::ParameterName, 'omit')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;path;draft' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--name', '--name', [CompletionResultType]::ParameterName, 'name')
            [CompletionResult]::new('--label', '--label', [CompletionResultType]::ParameterName, 'label')
            [CompletionResult]::new('--via', '--via', [CompletionResultType]::ParameterName, 'via')
            [CompletionResult]::new('--omit', '--omit', [CompletionResultType]::ParameterName, 'omit')
            [CompletionResult]::new('--by', '--by', [CompletionResultType]::ParameterName, 'by')
            [CompletionResult]::new('--dead-end', '--dead-end', [CompletionResultType]::ParameterName, 'dead-end')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;align' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('propose', 'propose', [CompletionResultType]::ParameterValue, 'Propose scored alignments between two named graphs as an SSSOM set')
            [CompletionResult]::new('decide', 'decide', [CompletionResultType]::ParameterValue, 'Apply reviewer accept and negate decisions to a mapping set')
            [CompletionResult]::new('apply', 'apply', [CompletionResultType]::ParameterValue, 'Materialize decided alignments into a derived alignment graph')
            break
        }
        'quipu;align;propose' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--set-id', '--set-id', [CompletionResultType]::ParameterName, 'set-id')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'out')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;align;decide' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--decisions', '--decisions', [CompletionResultType]::ParameterName, 'decisions')
            [CompletionResult]::new('--reviewer', '--reviewer', [CompletionResultType]::ParameterName, 'reviewer')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'out')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;align;apply' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph-a', '--graph-a', [CompletionResultType]::ParameterName, 'graph-a')
            [CompletionResult]::new('--graph-b', '--graph-b', [CompletionResultType]::ParameterName, 'graph-b')
            [CompletionResult]::new('--expected-version', '--expected-version', [CompletionResultType]::ParameterName, 'expected-version')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;propose' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--status', '--status', [CompletionResultType]::ParameterName, 'status')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List schema change proposals, optionally by status')
            [CompletionResult]::new('submit', 'submit', [CompletionResultType]::ParameterValue, 'Submit a schema change proposal')
            [CompletionResult]::new('accept', 'accept', [CompletionResultType]::ParameterValue, 'Accept a pending proposal')
            [CompletionResult]::new('reject', 'reject', [CompletionResultType]::ParameterValue, 'Reject a pending proposal')
            break
        }
        'quipu;propose;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--status', '--status', [CompletionResultType]::ParameterName, 'status')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;propose;submit' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--proposer', '--proposer', [CompletionResultType]::ParameterName, 'proposer')
            [CompletionResult]::new('--rationale', '--rationale', [CompletionResultType]::ParameterName, 'rationale')
            [CompletionResult]::new('--trigger', '--trigger', [CompletionResultType]::ParameterName, 'trigger')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;propose;accept' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--decided-by', '--decided-by', [CompletionResultType]::ParameterName, 'decided-by')
            [CompletionResult]::new('--note', '--note', [CompletionResultType]::ParameterName, 'note')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;propose;reject' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--decided-by', '--decided-by', [CompletionResultType]::ParameterName, 'decided-by')
            [CompletionResult]::new('--note', '--note', [CompletionResultType]::ParameterName, 'note')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;memory' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('read', 'read', [CompletionResultType]::ParameterValue, 'Read a versioned memory file for an owner')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export an owner''s memory files as a file-data envelope')
            [CompletionResult]::new('receipt', 'receipt', [CompletionResultType]::ParameterValue, 'Compute the receipt a memory commit request would produce')
            [CompletionResult]::new('commit', 'commit', [CompletionResultType]::ParameterValue, 'Commit a versioned memory file change')
            break
        }
        'quipu;memory;read' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;memory;export' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;memory;receipt' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;memory;commit' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--local-authority', '--local-authority', [CompletionResultType]::ParameterName, 'local-authority')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;attest' {
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
        'quipu;attest;register' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--agent', '--agent', [CompletionResultType]::ParameterName, 'agent')
            [CompletionResult]::new('--session', '--session', [CompletionResultType]::ParameterName, 'session')
            [CompletionResult]::new('--public-key', '--public-key', [CompletionResultType]::ParameterName, 'public-key')
            [CompletionResult]::new('--introducer', '--introducer', [CompletionResultType]::ParameterName, 'introducer')
            [CompletionResult]::new('--issued-at', '--issued-at', [CompletionResultType]::ParameterName, 'issued-at')
            [CompletionResult]::new('--expires-at', '--expires-at', [CompletionResultType]::ParameterName, 'expires-at')
            [CompletionResult]::new('--allow-write', '--allow-write', [CompletionResultType]::ParameterName, 'allow-write')
            [CompletionResult]::new('--require-v2', '--require-v2', [CompletionResultType]::ParameterName, 'require-v2')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;attest;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;attest;allow-write' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;attest;deny-write' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;attest;require-v2' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;attest;allow-v1' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;attest;revoke' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;signer' {
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
        'quipu;signer;enroll' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--credential-id', '--credential-id', [CompletionResultType]::ParameterName, 'credential-id')
            [CompletionResult]::new('--cose-key', '--cose-key', [CompletionResultType]::ParameterName, 'cose-key')
            [CompletionResult]::new('--scheme', '--scheme', [CompletionResultType]::ParameterName, 'scheme')
            [CompletionResult]::new('--origin', '--origin', [CompletionResultType]::ParameterName, 'origin')
            [CompletionResult]::new('--rp-id', '--rp-id', [CompletionResultType]::ParameterName, 'rp-id')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--expires-at', '--expires-at', [CompletionResultType]::ParameterName, 'expires-at')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;signer;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;signer;enable' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;signer;disable' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;signer;revoke' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;signer;protect-graph' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;deck' {
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
        'quipu;deck;configure' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--origin', '--origin', [CompletionResultType]::ParameterName, 'origin')
            [CompletionResult]::new('--rp-id', '--rp-id', [CompletionResultType]::ParameterName, 'rp-id')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--author', '--author', [CompletionResultType]::ParameterName, 'author')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;deck;enroll' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--credential-id', '--credential-id', [CompletionResultType]::ParameterName, 'credential-id')
            [CompletionResult]::new('--cose-key', '--cose-key', [CompletionResultType]::ParameterName, 'cose-key')
            [CompletionResult]::new('--scheme', '--scheme', [CompletionResultType]::ParameterName, 'scheme')
            [CompletionResult]::new('--expires-at', '--expires-at', [CompletionResultType]::ParameterName, 'expires-at')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;deck;enable' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;deck;disable' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;deck;revoke' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;deck;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;trust-root' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('challenge', 'challenge', [CompletionResultType]::ParameterValue, 'Issue the proof-of-possession challenge for a first verifier key')
            [CompletionResult]::new('bootstrap', 'bootstrap', [CompletionResultType]::ParameterValue, 'Enroll the first verifier key with its signed challenge')
            [CompletionResult]::new('status', 'status', [CompletionResultType]::ParameterValue, 'Show the root-of-trust state')
            break
        }
        'quipu;trust-root;challenge' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verifier', '--verifier', [CompletionResultType]::ParameterName, 'verifier')
            [CompletionResult]::new('--public-key', '--public-key', [CompletionResultType]::ParameterName, 'public-key')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;trust-root;bootstrap' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verifier', '--verifier', [CompletionResultType]::ParameterName, 'verifier')
            [CompletionResult]::new('--public-key', '--public-key', [CompletionResultType]::ParameterName, 'public-key')
            [CompletionResult]::new('--pop-signature', '--pop-signature', [CompletionResultType]::ParameterName, 'pop-signature')
            [CompletionResult]::new('--attests', '--attests', [CompletionResultType]::ParameterName, 'attests')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;trust-root;status' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph' {
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
        'quipu;graph;create' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'timestamp')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph;label' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--label', '--label', [CompletionResultType]::ParameterName, 'label')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'timestamp')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--kind', '--kind', [CompletionResultType]::ParameterName, 'kind')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--frozen', '--frozen', [CompletionResultType]::ParameterName, 'frozen')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph;import' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--as', '--as', [CompletionResultType]::ParameterName, 'as')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph;freeze' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'out')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph;thaw' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--actor', '--actor', [CompletionResultType]::ParameterName, 'actor')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph;overlay-create' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--input', '--input', [CompletionResultType]::ParameterName, 'input')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph;overlay-write' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--input', '--input', [CompletionResultType]::ParameterName, 'input')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;graph;overlay-compose' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--input', '--input', [CompletionResultType]::ParameterName, 'input')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;db' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('respace', 'respace', [CompletionResultType]::ParameterValue, 'Copy the store into a term space')
            [CompletionResult]::new('attach', 'attach', [CompletionResultType]::ParameterValue, 'List the databases mounted alongside this store')
            [CompletionResult]::new('migrate-inferred', 'migrate-inferred', [CompletionResultType]::ParameterValue, 'Move engine-derived facts into their companion inferred graphs')
            break
        }
        'quipu;db;respace' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--into', '--into', [CompletionResultType]::ParameterName, 'into')
            [CompletionResult]::new('--out', '--out', [CompletionResultType]::ParameterName, 'out')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;db;attach' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--list', '--list', [CompletionResultType]::ParameterName, 'list')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;db;migrate-inferred' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;events' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('refusals', 'refusals', [CompletionResultType]::ParameterValue, 'List recorded write refusals')
            break
        }
        'quipu;events;refusals' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;fork' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--name', '--name', [CompletionResultType]::ParameterName, 'name')
            [CompletionResult]::new('--timestamp', '--timestamp', [CompletionResultType]::ParameterName, 'timestamp')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List named forks')
            [CompletionResult]::new('diff', 'diff', [CompletionResultType]::ParameterValue, 'Diff two forks')
            [CompletionResult]::new('drop', 'drop', [CompletionResultType]::ParameterValue, 'Drop a named fork')
            [CompletionResult]::new('promote', 'promote', [CompletionResultType]::ParameterValue, 'Promote a fork to ROOT')
            break
        }
        'quipu;fork;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;fork;diff' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;fork;drop' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;fork;promote' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;audit' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--repo', '--repo', [CompletionResultType]::ParameterName, 'repo')
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'from')
            [CompletionResult]::new('--to', '--to', [CompletionResultType]::ParameterName, 'to')
            [CompletionResult]::new('--yupana', '--yupana', [CompletionResultType]::ParameterName, 'yupana')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
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
        'quipu;audit;inventory' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;audit;namespace' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;audit;replay' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--delta', '--delta', [CompletionResultType]::ParameterName, 'delta')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;audit;tree' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;audit;inheritance' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;audit;quarantine' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verdict', '--verdict', [CompletionResultType]::ParameterName, 'verdict')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List refused-attempt evidence, optionally for one verdict')
            [CompletionResult]::new('purge', 'purge', [CompletionResultType]::ParameterValue, 'Purge quarantined evidence by graph, age or all')
            break
        }
        'quipu;audit;quarantine;list' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--verdict', '--verdict', [CompletionResultType]::ParameterName, 'verdict')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'quipu;audit;quarantine;purge' {
            [CompletionResult]::new('--db', '--db', [CompletionResultType]::ParameterName, 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)')
            [CompletionResult]::new('--graph', '--graph', [CompletionResultType]::ParameterName, 'graph')
            [CompletionResult]::new('--before', '--before', [CompletionResultType]::ParameterName, 'before')
            [CompletionResult]::new('--older-than', '--older-than', [CompletionResultType]::ParameterName, 'older-than')
            [CompletionResult]::new('--all', '--all', [CompletionResultType]::ParameterName, 'all')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'json')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
