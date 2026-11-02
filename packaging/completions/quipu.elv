
use builtin;
use str;

set edit:completion:arg-completer[quipu] = {|@words|
    fn spaces {|n|
        builtin:repeat $n ' ' | str:join ''
    }
    fn cand {|text desc|
        edit:complex-candidate $text &display=$text' '(spaces (- 14 (wcswidth $text)))$desc
    }
    var command = 'quipu'
    for word $words[1..-1] {
        if (str:has-prefix $word '-') {
            break
        }
        set command = $command';'$word
    }
    var completions = [
        &'quipu'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help (see more with ''--help'')'
            cand --help 'Print help (see more with ''--help'')'
            cand store 'Inspect native ownership, or explicitly adopt or restore a disk store'
            cand version 'Print the version and source commit'
            cand install-alias 'Add the qp alias beside the quipu binary'
            cand knot 'Assert Turtle facts, validated against loaded shapes'
            cand load 'Assert Turtle facts (alias of knot), or load a project bundle directory'
            cand ingest 'Bulk-load an N-Triples, Turtle or N-Quads file into one graph with declared count and digest'
            cand read 'Run a SPARQL query, optionally as of a valid time or transaction'
            cand query 'Run a SPARQL query (alias of read)'
            cand rule-query 'Read typed ROOT support and the admitted judgment for a rule request'
            cand cord 'List entities, optionally of one type'
            cand unravel 'Show facts as they were at a transaction or valid time'
            cand impact 'Walk what depends on an entity, optionally after a speculative removal'
            cand explain 'Trace a derived fact back to its premises'
            cand project 'Project the graph and run a graph algorithm such as PageRank'
            cand report 'Report hubs, surprising links and open questions in the graph'
            cand reason 'Run Datalog rules to a fixpoint and persist the derivations'
            cand demotions 'Read preserved unsupported demotions without materializing a closure'
            cand episode 'Ingest a structured episode of nodes and edges from JSON'
            cand retract 'Retract an entity''s facts, optionally one predicate; history is kept'
            cand shapes 'Load, list or remove the SHACL shapes that guard writes'
            cand namespaces 'List or apply the owner namespace registry'
            cand entity-ref 'Mint or resolve stable entity references'
            cand ontology 'Load, list or remove OWL ontologies (owl feature)'
            cand validate 'Validate a data file against a shapes file without writing'
            cand repl 'Open an interactive SPARQL prompt'
            cand export 'Export RDF from ROOT, one graph, a provenance group or a CONSTRUCT query'
            cand databook 'Export a graph as a DataBook, or import a DataBook''s graph data'
            cand render 'Render graph data into files in an owned directory'
            cand stats 'Show store statistics'
            cand prime 'Print the agent primer and a bounded, read-only summary of the store'
            cand search 'Search entities by keyword, vector or hybrid ranking'
            cand search-index 'Report, backfill or drop the search indexes'
            cand doctor 'Report graph-label drift and recommended floors'
            cand pack 'Export a graph, or the whole store, as a pack; or verify a pack'
            cand unpack 'Merge a published pack into a local graph'
            cand restore 'Replace the store with a full pack, binary or text'
            cand share 'Write a portable share of a graph, scope or project'
            cand status 'Report how a share has diverged from this store'
            cand merge 'Merge a share back into ROOT, with reviewable conflict decisions'
            cand import 'Verify a share into quarantine, then promote or review it'
            cand compose 'Compose verified packs into a named inspection dataset'
            cand changes 'Read whole transaction change pages from the change feed'
            cand follow 'Keep this database a read-only follower of a primary server'
            cand acquire 'Measure the bounded scope of a direct-main graph acquisition'
            cand migrate-vectors 'Move stored vectors between backends (lancedb feature)'
            cand git-merge 'Merge pendants from Git snapshots and stop before committing'
            cand merge-driver 'Low-level Git merge driver for pendant files'
            cand pendant-resolve 'Record a resolution choice for one pendant merge conflict'
            cand pendant-check 'Check a pendant merge result in CI without a merge driver'
            cand diff-textconv 'Render a pack with labels for git diff textconv'
            cand mcp 'Serve the MCP tools over stdio through the sibling quipu-server'
            cand hook 'Run a harness hook such as session capture'
            cand hooks 'Bundle, install, uninstall or check quipu''s Claude Code and Codex hooks'
            cand policy 'Draft an advisory policy from an exemplar, or backtest one'
            cand gate 'Judge a candidate policy set over recorded history without writing'
            cand path 'Golden-path analysis: provenance cone, backtest and draft'
            cand align 'Propose, decide and apply cross-graph alignments'
            cand propose 'List, submit, accept or reject schema change proposals'
            cand memory 'Read, export, receipt or commit versioned file memory'
            cand attest 'Register and manage attested session bindings'
            cand signer 'Enroll and manage hardware signers for attested writes'
            cand deck 'Configure and enroll the Decision Deck'
            cand trust-root 'Enroll the first human key of the root of trust'
            cand graph 'Create, label, list, import, freeze and thaw named graphs and overlays'
            cand db 'Respace, attach or migrate the store file'
            cand events 'Read store event records such as refusals'
            cand fork 'Create, list, diff, drop or promote named forks of ROOT'
            cand audit 'Check traces, namespaces, replays and quarantine against policy'
        }
        &'quipu;store'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand status 'Report inactive or enforced native store ownership without adopting'
            cand adopt 'Bind an existing native store to this host and path with a fresh epoch'
            cand restore 'Restore a SQLite snapshot to a new path and explicitly adopt it'
        }
        &'quipu;store;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;store;adopt'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;store;restore'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;version'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;install-alias'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --bin-dir 'Directory containing the CLI alias'
            cand --check 'Check the alias without installing it'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;knot'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --shapes 'Shapes used for validation or packaging'
            cand --graph 'Explicit graph IRI for this operation'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand --blank-node-scope 'Blank-node identity scope for this import'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;load'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --shapes 'Shapes used for validation or packaging'
            cand --graph 'Explicit graph IRI for this operation'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand --blank-node-scope 'Blank-node identity scope for this import'
            cand --destination 'Publication destination evaluated by policy'
            cand --actor 'Actor recorded in operation provenance'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;ingest'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand --declare-count 'Expected ingest triple count'
            cand --declare-sha256 'Expected input SHA-256 digest'
            cand --format 'Output or input format for this operation'
            cand --chunk 'Maximum triples per ingest chunk'
            cand --base-iri 'Base IRI for relative RDF references'
            cand --actor 'Actor recorded in operation provenance'
            cand --source 'Source identifier recorded in provenance'
            cand --blank-node-scope 'Blank-node identity scope for this import'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;read'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --valid-at 'Valid-time instant to query'
            cand --tx 'Historical transaction to read'
            cand --fork 'Named store fork to query'
            cand --entailment 'Query entailment regime'
            cand --federation 'Enable configured remote query federation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;query'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --valid-at 'Valid-time instant to query'
            cand --tx 'Historical transaction to read'
            cand --fork 'Named store fork to query'
            cand --entailment 'Query entailment regime'
            cand --federation 'Enable configured remote query federation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;rule-query'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;cord'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --type 'Entity type IRI to select'
            cand --limit 'Maximum number of results'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;unravel'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --tx 'Historical transaction to read'
            cand --valid-at 'Valid-time instant to query'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;impact'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --hops 'Maximum traversal hops'
            cand --predicate 'Predicate IRI used to filter facts or edges'
            cand --remove 'Calculate the impact of removing the entity'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;explain'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --depth 'Maximum explanation depth'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;project'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --algorithm 'Graph projection algorithm'
            cand --type 'Entity type IRI to select'
            cand --predicate 'Predicate IRI used to filter facts or edges'
            cand --graph 'Explicit graph IRI for this operation'
            cand --limit 'Maximum number of results'
            cand --damping 'PageRank damping factor'
            cand --max-iters 'Maximum algorithm iterations'
            cand --tolerance 'Convergence tolerance for the algorithm'
            cand --from 'Start entity IRI for shortest-path projection'
            cand --to 'Destination entity IRI for shortest-path projection'
            cand --seed 'Seed entity for graph projection'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;report'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --type 'Entity type IRI to select'
            cand --predicate 'Predicate IRI used to filter facts or edges'
            cand --hubs 'Number of hubs in the report'
            cand --surprises 'Number of surprising relationships in the report'
            cand --questions 'Number of suggested questions in the report'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;reason'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --rules 'Rule set to load or evaluate'
            cand --reactive 'Use the reactive reasoner'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;demotions'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;episode'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --base-ns 'Base namespace for episode entities'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;retract'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --predicate 'Predicate IRI used to filter facts or edges'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;shapes'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List the loaded SHACL shapes'
            cand load 'Load a named SHACL shapes file; later writes are validated against it'
            cand remove 'Remove a named shapes file'
        }
        &'quipu;shapes;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;shapes;load'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;shapes;remove'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;namespaces'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List the effective owner namespace registry'
            cand apply 'Apply the configured owner namespace registry to the store'
        }
        &'quipu;namespaces;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;namespaces;apply'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;entity-ref'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand mint 'Mint a typed stable entity reference from a JSON request'
            cand resolve 'Resolve a stable entity reference from a JSON request'
            cand verify 'Verify a current key IRI against its local key triples'
        }
        &'quipu;entity-ref;mint'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;entity-ref;resolve'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;entity-ref;verify'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;ontology'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List loaded OWL ontologies'
            cand load 'Load an OWL ontology and materialize its entailments'
            cand remove 'Remove a loaded OWL ontology'
        }
        &'quipu;ontology;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;ontology;load'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;ontology;remove'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;validate'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --shapes 'Shapes used for validation or packaging'
            cand --data 'RDF data file to validate'
            cand --format 'Output or input format for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;repl'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;export'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand --group-id 'Select facts belonging to this group'
            cand --construct 'SPARQL CONSTRUCT query selecting the share'
            cand --format 'Output or input format for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;databook'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand export 'Write one graph, current or as of a past point, as a DataBook'
            cand import 'Load a DataBook''s graph blocks in one write; never runs its queries'
        }
        &'quipu;databook;export'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand --tx 'tx'
            cand --valid-at 'valid-at'
            cand --id 'id'
            cand --title 'title'
            cand --version 'version'
            cand --timestamp 'timestamp'
            cand --out 'out'
            cand --with-shapes 'with-shapes'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;databook;import'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand --timestamp 'timestamp'
            cand --actor 'actor'
            cand --source 'source'
            cand --with-shapes 'with-shapes'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;render'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --dest 'Render output destination'
            cand --graph 'Explicit graph IRI for this operation'
            cand --valid-at 'Valid-time instant to query'
            cand --server 'Render service URL'
            cand --query 'Query used for rendering'
            cand --template 'Render template'
            cand --layout 'Render layout'
            cand --path 'Relative output path template'
            cand --dry-run 'Preview the operation without applying it'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;stats'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;prime'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --section 'Agent primer section to print'
            cand --brief 'Print the short agent primer'
            cand --static 'Print the embedded primer without live context'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;search'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --mode 'Operation mode'
            cand --limit 'Maximum number of results'
            cand --alpha 'Hybrid search semantic/keyword weighting'
            cand --rrf-k 'Reciprocal-rank fusion constant'
            cand --decay 'Search recency decay factor'
            cand --anchor 'Entity IRI anchoring graph-aware search'
            cand --anchor-mode 'How the anchor affects candidate selection'
            cand --direction 'Traversal direction for graph-aware search'
            cand --max-hops 'Maximum graph-search traversal hops'
            cand --via 'Predicate or path used for traversal'
            cand --fusion 'Search result fusion strategy'
            cand --valid-at 'Valid-time instant to query'
            cand --type 'Entity type IRI to select'
            cand --group 'Group identifier for the operation'
            cand --structured-query 'Structured search query as JSON'
            cand --filters 'Structured search filters as JSON'
            cand --graph 'Explicit graph IRI for this operation'
            cand --graphs 'Explicit graph selection for search'
            cand --explain 'Include search ranking explanations'
            cand --all-graphs 'Search all permitted graphs explicitly'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;search-index'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'Index or graph kind to select'
            cand --batch-size 'Number of entries per indexing batch'
            cand -h 'Print help'
            cand --help 'Print help'
            cand status 'Report the state of a search index'
            cand backfill 'Backfill a search index in batches'
            cand drop 'Drop a search index'
        }
        &'quipu;search-index;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'Index or graph kind to select'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;search-index;backfill'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'Index or graph kind to select'
            cand --batch-size 'Number of entries per indexing batch'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;search-index;drop'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'Index or graph kind to select'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;doctor'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand labels 'Report graph-label drift and recommended floors'
        }
        &'quipu;doctor;labels'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;pack'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verify 'Pack file to verify without creating a new pack'
            cand --out 'Output file'
            cand --space 'Namespace space recorded in pack metadata'
            cand --name 'Name of the generated object'
            cand --version 'Version recorded in pack metadata'
            cand --shapes 'Shapes used for validation or packaging'
            cand --queries 'Stored queries to include'
            cand --repo 'Repository identity or source root for this operation'
            cand --repo-sha 'Repository commit recorded in pack provenance'
            cand --model-id 'Embedding model identifier'
            cand --model-version 'Embedding model version'
            cand --destination 'Publication destination evaluated by policy'
            cand --format 'Output or input format for this operation'
            cand --with-vectors 'Include embedding vectors in the pack'
            cand --full 'Include the full store or full primer, as applicable'
            cand --allow-missing-embedding-recipe 'Allow a pack without the embedding provider recipe'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;unpack'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --into 'Target store for unpacking'
            cand --expect-repo 'Expected repository identity in the pack'
            cand --head-sha 'Expected repository head commit'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;restore'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --force 'Permit replacement when restoring a store'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;share'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --output 'Output directory for the share'
            cand --graph 'Explicit graph IRI for this operation'
            cand --group-id 'Select facts belonging to this group'
            cand --construct 'SPARQL CONSTRUCT query selecting the share'
            cand --shapes 'Shapes used for validation or packaging'
            cand --queries 'Stored queries to include'
            cand --alignments 'Reviewed SSSOM/RDF alignment file to carry'
            cand --parent-share 'Parent share for a delta export'
            cand --since 'Starting share or transaction for incremental output'
            cand --destination 'Publication destination evaluated by policy'
            cand --pack-dir 'Existing pack directory to share'
            cand --actor 'Actor recorded in operation provenance'
            cand --attest-key 'Ed25519 attestation signing key file'
            cand --attest-issued-at 'Attestation issue time as Unix seconds'
            cand --attest-ttl 'Attestation lifetime in seconds'
            cand --attest-agent 'Attested agent identifier'
            cand --attest-session 'Attested session identifier'
            cand --attest-introducer 'Attestation introducer identifier'
            cand --attest-nonce 'Unique attestation nonce'
            cand --pointers 'TOML file declaring external graph pointers'
            cand --no-shapes 'Omit shapes from the share'
            cand --no-queries 'Omit stored queries from the share'
            cand --turtle 'Include a human-readable Turtle export'
            cand --attest 'Attach a signed agent attestation'
            cand --project 'Export this repository''s project graph to .quipu/graph'
            cand -h 'Print help'
            cand --help 'Print help'
            cand diff 'Diff two packs by entity, or write a PR-review report'
        }
        &'quipu;share;diff'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --format 'Output or input format for this operation'
            cand --old-shapes 'Shapes used to validate the old share'
            cand --new-shapes 'Shapes used to validate the new share'
            cand --decisions 'Conflict decisions file'
            cand --report 'Print the validation diff report'
            cand --fail-on-introduced 'Exit nonzero when the diff introduces violations'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;merge'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'Actor recorded in operation provenance'
            cand --emit-decisions 'Write merge decisions to this file'
            cand --decisions 'Conflict decisions file'
            cand --reviewer 'Identity reviewing the merge'
            cand --slot-scoped 'Restrict the merge to selected slots'
            cand --propose 'Produce a merge proposal without applying it'
            cand --dry-run 'Preview the operation without applying it'
            cand -h 'Print help'
            cand --help 'Print help'
            cand show 'Show a merge''s conflicts in the standard layout'
            cand resolve 'Record one choice on one merge conflict'
        }
        &'quipu;merge;show'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --format 'Output or input format for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;merge;resolve'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --choose 'Conflict side to choose'
            cand --value 'Explicit value for a conflict resolution'
            cand --by 'Identity recording a resolution'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;import'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --source 'Source identifier recorded in provenance'
            cand --actor 'Actor recorded in operation provenance'
            cand --destination 'Publication destination evaluated by policy'
            cand --query-namespace 'Namespace for imported stored queries'
            cand --replace-queries 'Replace imported query definitions on name conflicts'
            cand -h 'Print help'
            cand --help 'Print help'
            cand delta 'Verify a delta share against its parent share'
            cand promote 'Promote a staged share into the store'
            cand review 'Review staged shares awaiting promotion'
        }
        &'quipu;import;delta'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'Actor recorded in operation provenance'
            cand --destination 'Publication destination evaluated by policy'
            cand --query-namespace 'Namespace for imported stored queries'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;import;promote'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'Actor recorded in operation provenance'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;import;review'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand pending 'List staged shares awaiting review'
            cand notify 'Report staged shares older than an age threshold to a route'
            cand rejected 'Mark a staged share rejected, with a reason'
            cand expired 'Mark a staged share expired, with a reason'
            cand reopen 'Reopen a rejected or expired staged share'
        }
        &'quipu;import;review;pending'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --after 'Resume after this cursor'
            cand --limit 'Maximum number of results'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;import;review;notify'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --after 'Resume after this cursor'
            cand --limit 'Maximum number of results'
            cand --age-seconds 'Minimum notification age in seconds'
            cand --route 'Notification route for pending imports'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;import;review;rejected'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'Actor recorded in operation provenance'
            cand --reason 'Reason recorded for the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;import;review;expired'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'Actor recorded in operation provenance'
            cand --reason 'Reason recorded for the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;import;review;reopen'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'Actor recorded in operation provenance'
            cand --reason 'Reason recorded for the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;compose'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --repo 'Repository identity or source root for this operation'
            cand --shapes-from 'Pack supplying the composition shapes'
            cand --actor 'Actor recorded in operation provenance'
            cand --destination 'Publication destination evaluated by policy'
            cand --revoke-tombstone 'Tombstone triple to revoke in the composition'
            cand --pack-key 'Composition pack key'
            cand --reason 'Reason recorded for the operation'
            cand --use-pointer 'Pointer to use in the composition'
            cand --declaration 'Composition declaration file'
            cand --authority 'Authority identifier for the operation'
            cand --repo-remote 'Repository remote used to resolve a pointer'
            cand --pointer-cache 'Local directory for resolved pack pointers'
            cand --rollback 'Restore the previous CURRENT composition snapshot'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;changes'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --from 'First transaction to include'
            cand --capture 'Change-feed row capture mode'
            cand --limit 'Maximum number of results'
            cand --graph 'Explicit graph IRI for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;follow'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --primary 'Primary server URL to follow'
            cand --page-txs 'Transactions per follower page'
            cand --page-gap-ms 'Delay between follower pages in milliseconds'
            cand --bootstrap-only 'Bootstrap a follower and exit without polling'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;acquire'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand scope 'Report the bounded row and byte scope of a direct-main acquisition'
        }
        &'quipu;acquire;scope'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --source-store-id 'Identity of the source store being acquired'
            cand --graph 'Explicit graph IRI for this operation'
            cand --tx 'Historical transaction to read'
            cand --max-vm-steps 'Maximum SPARQL virtual-machine steps'
            cand --deadline-ms 'Acquisition query deadline in milliseconds'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;migrate-vectors'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --from 'Source vector backend'
            cand --to 'Destination vector backend'
            cand --dry-run 'Preview the operation without applying it'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;git-merge'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand show 'Show a qpack''s pending merge decisions in the standard layout'
            cand resolve 'Record one choice in the Git merge in progress and rewrite the qpack'
        }
        &'quipu;git-merge;show'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --format 'Output or input format for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;git-merge;resolve'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --choose 'Conflict side to choose'
            cand --by 'Identity recording a resolution'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;merge-driver'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;pendant-resolve'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;pendant-check'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --history 'Check the pendant merge history'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;diff-textconv'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;mcp'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --mcp-token-file 'File containing the MCP authentication token'
            cand --bind 'MCP HTTP listen address'
            cand --primary-url 'Primary server URL used by a read-only MCP follower'
            cand --read-only 'Serve MCP in read-only mode'
            cand --embed-backfill 'Enable embedding backfill in the MCP server'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;hook'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand session-capture 'Stop hook: solicit one knowledge episode per session from stdin JSON'
        }
        &'quipu;hook;session-capture'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;hooks'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand bundle 'Print quipu''s hook bundle (st.hook-bundle/1)'
            cand install 'Install quipu''s hooks into Claude Code or Codex'
            cand uninstall 'Remove quipu''s hooks from Claude Code or Codex'
            cand status 'Report whether quipu''s hooks are installed'
        }
        &'quipu;hooks;bundle'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;hooks;install'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --harness 'Agent harness: claude or codex'
            cand --project 'Use this repository''s project graph or project-local hooks'
            cand --no-st 'Do not integrate hooks with the crew orchestrator'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;hooks;uninstall'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --harness 'Agent harness: claude or codex'
            cand --project 'Use this repository''s project graph or project-local hooks'
            cand --no-st 'Do not integrate hooks with the crew orchestrator'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;hooks;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --harness 'Agent harness: claude or codex'
            cand --project 'Use this repository''s project graph or project-local hooks'
            cand --no-st 'Do not integrate hooks with the crew orchestrator'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;policy'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand draft 'Draft advisory policy Turtle from an exemplar and an intent'
            cand backtest 'Backtest a candidate policy over recorded transactions'
        }
        &'quipu;policy;draft'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --exemplar 'Source exemplar for a policy draft'
            cand --name 'Name of the generated object'
            cand --label 'Human-readable label for the draft'
            cand --targets 'Target class IRI for the policy'
            cand --claim 'ASK query expressing the proposed policy claim'
            cand --class 'Policy classification: soft or hard'
            cand --point 'Policy verification point'
            cand --layer 'Policy enforcement layer'
            cand --authority 'Authority identifier for the operation'
            cand --out 'Output file'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;policy;backtest'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --last-txs 'Number of recent transactions to evaluate'
            cand --from-tx 'First transaction in the selected interval'
            cand --to-tx 'Last transaction in the selected interval'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;gate'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand shadow 'Judge a candidate policy set over recorded history on a quiescent copy'
        }
        &'quipu;gate;shadow'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --rules 'Rule set to load or evaluate'
            cand --since 'Starting share or transaction for incremental output'
            cand --from-tx 'First transaction in the selected interval'
            cand --to-tx 'Last transaction in the selected interval'
            cand --last-txs 'Number of recent transactions to evaluate'
            cand --mode 'Operation mode'
            cand --max-txs 'Maximum transactions to evaluate'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;path'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand cone 'Compute the provenance cone of a trajectory'
            cand backtest 'Backtest a pruned trajectory over recorded history'
            cand draft 'Draft a golden-path policy from a trajectory as Turtle for review'
        }
        &'quipu;path;cone'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --via 'Predicate or path used for traversal'
            cand --hops 'Maximum traversal hops'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;path;backtest'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --omit 'Trajectory step to omit in a counterfactual'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;path;draft'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --name 'Name of the generated object'
            cand --label 'Human-readable label for the draft'
            cand --via 'Predicate or path used for traversal'
            cand --omit 'Trajectory step to omit in a counterfactual'
            cand --by 'Identity recording a resolution'
            cand --dead-end 'Dead-end trajectory to include in the policy draft'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;align'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand propose 'Propose scored alignments between two named graphs as an SSSOM set'
            cand decide 'Apply reviewer accept and negate decisions to a mapping set'
            cand apply 'Materialize decided alignments into a derived alignment graph'
        }
        &'quipu;align;propose'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --set-id 'Identifier for the proposed alignment set'
            cand --out 'Output file'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;align;decide'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --decisions 'Conflict decisions file'
            cand --reviewer 'Identity reviewing the merge'
            cand --out 'Output file'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;align;apply'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph-a 'First graph in the alignment pair'
            cand --graph-b 'Second graph in the alignment pair'
            cand --expected-version 'Expected version for the compare-and-swap operation'
            cand --actor 'Actor recorded in operation provenance'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;propose'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --status 'Status to record or filter'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List schema change proposals, optionally by status'
            cand submit 'Submit a schema change proposal'
            cand accept 'Accept a pending proposal'
            cand reject 'Reject a pending proposal'
        }
        &'quipu;propose;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --status 'Status to record or filter'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;propose;submit'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --proposer 'Identity proposing an alignment'
            cand --rationale 'Reason for the alignment decision'
            cand --trigger 'Trigger identifier'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;propose;accept'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --decided-by 'Identity recording the alignment decision'
            cand --note 'Explanatory note recorded with the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;propose;reject'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --decided-by 'Identity recording the alignment decision'
            cand --note 'Explanatory note recorded with the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;memory'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand read 'Read a versioned memory file for an owner'
            cand export 'Export an owner''s memory files as a file-data envelope'
            cand receipt 'Compute the receipt a memory commit request would produce'
            cand commit 'Commit a versioned memory file change'
        }
        &'quipu;memory;read'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;memory;export'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;memory;receipt'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;memory;commit'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand --local-authority 'Explicitly authorize a local memory commit'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;attest'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand register 'Register a session binding for attested writes'
            cand list 'List registered session bindings'
            cand allow-write 'Allow a session to write'
            cand deny-write 'Stop a session from writing'
            cand require-v2 'Require write-v2 signatures for a session'
            cand allow-v1 'Allow write-v1 signatures for a session again'
            cand revoke 'Revoke a session binding'
        }
        &'quipu;attest;register'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --agent 'Agent identifier for the binding'
            cand --session 'Session identifier on the statement'
            cand --public-key 'Verifier public key'
            cand --introducer 'Identifier of the binding introducer'
            cand --issued-at 'Issue time as Unix seconds'
            cand --expires-at 'Expiration time as Unix seconds'
            cand --allow-write 'Grant write permission to the attested binding'
            cand --require-v2 'Require version 2 attestations'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;attest;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;attest;allow-write'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;attest;deny-write'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;attest;require-v2'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;attest;allow-v1'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;attest;revoke'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;signer'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand enroll 'Enroll a hardware signer credential'
            cand status 'Show signer enrollment and enforcement state'
            cand enable 'Enable signer enforcement'
            cand disable 'Disable signer enforcement'
            cand revoke 'Revoke a signer credential'
            cand protect-graph 'Permanently require attested writes for a graph'
        }
        &'quipu;signer;enroll'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --credential-id 'Base64url-encoded WebAuthn credential ID'
            cand --cose-key 'Base64url-encoded COSE public key'
            cand --scheme 'Signature scheme'
            cand --origin 'WebAuthn origin URL'
            cand --rp-id 'WebAuthn relying-party ID'
            cand --actor 'Actor recorded in operation provenance'
            cand --graph 'Explicit graph IRI for this operation'
            cand --expires-at 'Expiration time as Unix seconds'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;signer;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;signer;enable'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;signer;disable'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;signer;revoke'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;signer;protect-graph'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;deck'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand configure 'Configure the Decision Deck scope (starts disabled)'
            cand enroll 'Enroll a Decision Deck credential'
            cand enable 'Enable the Decision Deck'
            cand disable 'Disable the Decision Deck'
            cand revoke 'Revoke a Decision Deck credential'
            cand status 'Show Decision Deck configuration and state'
        }
        &'quipu;deck;configure'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --origin 'WebAuthn origin URL'
            cand --rp-id 'WebAuthn relying-party ID'
            cand --actor 'Actor recorded in operation provenance'
            cand --author 'Author identifier on the signed statement'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;deck;enroll'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --credential-id 'Base64url-encoded WebAuthn credential ID'
            cand --cose-key 'Base64url-encoded COSE public key'
            cand --scheme 'Signature scheme'
            cand --expires-at 'Expiration time as Unix seconds'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;deck;enable'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;deck;disable'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;deck;revoke'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;deck;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;trust-root'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand challenge 'Issue the proof-of-possession challenge for a first verifier key'
            cand bootstrap 'Enroll the first verifier key with its signed challenge'
            cand status 'Show the root-of-trust state'
        }
        &'quipu;trust-root;challenge'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verifier 'Verifier identifier for a signed statement'
            cand --public-key 'Verifier public key'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;trust-root;bootstrap'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verifier 'Verifier identifier for a signed statement'
            cand --public-key 'Verifier public key'
            cand --pop-signature 'Proof-of-possession signature'
            cand --attests 'IRI of the object being attested'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;trust-root;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand create 'Register a committed named graph'
            cand label 'Declare lattice labels on a registered graph'
            cand list 'List registered named graphs'
            cand import 'Import another store file as a named graph'
            cand freeze 'Freeze a graph''s full history into a read-only archive pack'
            cand thaw 'Restore a frozen graph from its archive pack'
            cand overlay-create 'Create an overlay graph bound to a committed parent'
            cand overlay-write 'Assert, retract or tombstone a triple in an overlay'
            cand overlay-compose 'Resolve an overlay''s composed view over its parent'
        }
        &'quipu;graph;create'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'Actor recorded in operation provenance'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph;label'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --label 'Human-readable label for the draft'
            cand --actor 'Actor recorded in operation provenance'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'Index or graph kind to select'
            cand --graph 'Explicit graph IRI for this operation'
            cand --frozen 'List only frozen graphs'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph;import'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --as 'IRI under which to import the graph'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph;freeze'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --out 'Output file'
            cand --actor 'Actor recorded in operation provenance'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph;thaw'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'Actor recorded in operation provenance'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph;overlay-create'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --input 'Input file for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph;overlay-write'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --input 'Input file for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;graph;overlay-compose'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --input 'Input file for this operation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;db'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand respace 'Copy the store into a term space'
            cand attach 'List the databases mounted alongside this store'
            cand migrate-inferred 'Move engine-derived facts into their companion inferred graphs'
        }
        &'quipu;db;respace'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --into 'New integer namespace space number'
            cand --out 'Output file'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;db;attach'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --list 'List mounted read-only databases'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;db;migrate-inferred'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;events'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand refusals 'List recorded write refusals'
        }
        &'quipu;events;refusals'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;fork'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --name 'Name of the generated object'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List named forks'
            cand diff 'Diff two forks'
            cand drop 'Drop a named fork'
            cand promote 'Promote a fork to ROOT, with reviewable conflict decisions'
        }
        &'quipu;fork;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;fork;diff'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;fork;drop'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;fork;promote'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --timestamp 'Valid-time timestamp for the operation'
            cand --emit-decisions 'Write merge decisions to this file'
            cand --decisions 'Conflict decisions file'
            cand --reviewer 'Identity reviewing the merge'
            cand --actor 'Actor recorded in operation provenance'
            cand --propose 'Produce a merge proposal without applying it'
            cand --dry-run 'Preview the operation without applying it'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;audit'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --repo 'Repository identity or source root for this operation'
            cand --from 'Base Git revision for the audit'
            cand --to 'Tip Git revision for the audit'
            cand --yupana 'Yupana executable used for the audit'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
            cand inventory 'List the policies the audit checks against'
            cand namespace 'Report base-namespace drift'
            cand replay 'Re-derive a gate decision as of its transaction'
            cand tree 'Reassemble the dispatch forest from a trace''s principal chains'
            cand inheritance 'Check whether inherited constraints stopped applying under delegation'
            cand quarantine 'List refused-attempt evidence behind verdicts'
        }
        &'quipu;audit;inventory'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;audit;namespace'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;audit;replay'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --delta 'Refused-delta file for verdict replay'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;audit;tree'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;audit;inheritance'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;audit;quarantine'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verdict 'Verdict identifier to inspect'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List refused-attempt evidence, optionally for one verdict'
            cand purge 'Purge quarantined evidence by graph, age or all'
        }
        &'quipu;audit;quarantine;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verdict 'Verdict identifier to inspect'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'quipu;audit;quarantine;purge'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'Explicit graph IRI for this operation'
            cand --before 'Purge entries older than this timestamp'
            cand --older-than 'Minimum age for quarantine purging'
            cand --all 'Select all retained quarantine entries'
            cand --json 'Print machine-readable JSON'
            cand -h 'Print help'
            cand --help 'Print help'
        }
    ]
    $completions[$command]
}
