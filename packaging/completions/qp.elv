
use builtin;
use str;

set edit:completion:arg-completer[qp] = {|@words|
    fn spaces {|n|
        builtin:repeat $n ' ' | str:join ''
    }
    fn cand {|text desc|
        edit:complex-candidate $text &display=$text' '(spaces (- 14 (wcswidth $text)))$desc
    }
    var command = 'qp'
    for word $words[1..-1] {
        if (str:has-prefix $word '-') {
            break
        }
        set command = $command';'$word
    }
    var completions = [
        &'qp'= {
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
            cand propose 'List, submit, plan, accept or reject schema change proposals'
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
        &'qp;store'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand status 'Report inactive or enforced native store ownership without adopting'
            cand adopt 'Bind an existing native store to this host and path with a fresh epoch'
            cand restore 'Restore a SQLite snapshot to a new path and explicitly adopt it'
        }
        &'qp;store;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;store;adopt'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;store;restore'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;version'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;install-alias'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --bin-dir 'bin-dir'
            cand --check 'check'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;knot'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --shapes 'shapes'
            cand --graph 'graph'
            cand --timestamp 'timestamp'
            cand --blank-node-scope 'blank-node-scope'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;load'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --shapes 'shapes'
            cand --graph 'graph'
            cand --timestamp 'timestamp'
            cand --blank-node-scope 'blank-node-scope'
            cand --destination 'destination'
            cand --actor 'actor'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;ingest'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand --timestamp 'timestamp'
            cand --declare-count 'declare-count'
            cand --declare-sha256 'declare-sha256'
            cand --format 'format'
            cand --chunk 'chunk'
            cand --base-iri 'base-iri'
            cand --actor 'actor'
            cand --source 'source'
            cand --blank-node-scope 'blank-node-scope'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;read'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --valid-at 'valid-at'
            cand --tx 'tx'
            cand --fork 'fork'
            cand --entailment 'entailment'
            cand --federation 'federation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;query'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --valid-at 'valid-at'
            cand --tx 'tx'
            cand --fork 'fork'
            cand --entailment 'entailment'
            cand --federation 'federation'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;rule-query'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;cord'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --type 'type'
            cand --limit 'limit'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;unravel'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --tx 'tx'
            cand --valid-at 'valid-at'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;impact'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --hops 'hops'
            cand --predicate 'predicate'
            cand --remove 'remove'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;explain'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --depth 'depth'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;project'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --algorithm 'algorithm'
            cand --type 'type'
            cand --predicate 'predicate'
            cand --graph 'graph'
            cand --limit 'limit'
            cand --damping 'damping'
            cand --max-iters 'max-iters'
            cand --tolerance 'tolerance'
            cand --from 'from'
            cand --to 'to'
            cand --seed 'seed'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;report'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --type 'type'
            cand --predicate 'predicate'
            cand --hubs 'hubs'
            cand --surprises 'surprises'
            cand --questions 'questions'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;reason'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --rules 'rules'
            cand --reactive 'reactive'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;demotions'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;episode'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --base-ns 'base-ns'
            cand --timestamp 'timestamp'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;retract'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --predicate 'predicate'
            cand --timestamp 'timestamp'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;shapes'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List the loaded SHACL shapes'
            cand load 'Load a named SHACL shapes file; later writes are validated against it'
            cand remove 'Remove a named shapes file'
        }
        &'qp;shapes;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;shapes;load'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;shapes;remove'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;namespaces'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List the effective owner namespace registry'
            cand apply 'Apply the configured owner namespace registry to the store'
        }
        &'qp;namespaces;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;namespaces;apply'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;entity-ref'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand mint 'Mint a typed stable entity reference from a JSON request'
            cand resolve 'Resolve a stable entity reference from a JSON request'
            cand verify 'Verify a current key IRI against its local key triples'
        }
        &'qp;entity-ref;mint'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;entity-ref;resolve'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;entity-ref;verify'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;ontology'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List loaded OWL ontologies'
            cand load 'Load an OWL ontology and materialize its entailments'
            cand remove 'Remove a loaded OWL ontology'
        }
        &'qp;ontology;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;ontology;load'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;ontology;remove'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;validate'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --shapes 'shapes'
            cand --data 'data'
            cand --format 'format'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;repl'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;export'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand --group-id 'group-id'
            cand --construct 'construct'
            cand --format 'format'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;render'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --dest 'dest'
            cand --graph 'graph'
            cand --valid-at 'valid-at'
            cand --server 'server'
            cand --query 'query'
            cand --template 'template'
            cand --layout 'layout'
            cand --path 'path'
            cand --dry-run 'dry-run'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;stats'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;prime'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --section 'section'
            cand --brief 'brief'
            cand --static 'static'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;search'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --mode 'mode'
            cand --limit 'limit'
            cand --alpha 'alpha'
            cand --rrf-k 'rrf-k'
            cand --decay 'decay'
            cand --anchor 'anchor'
            cand --anchor-mode 'anchor-mode'
            cand --direction 'direction'
            cand --max-hops 'max-hops'
            cand --via 'via'
            cand --fusion 'fusion'
            cand --valid-at 'valid-at'
            cand --type 'type'
            cand --group 'group'
            cand --structured-query 'structured-query'
            cand --filters 'filters'
            cand --graph 'graph'
            cand --graphs 'graphs'
            cand --explain 'explain'
            cand --all-graphs 'all-graphs'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;search-index'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'kind'
            cand --batch-size 'batch-size'
            cand -h 'Print help'
            cand --help 'Print help'
            cand status 'Report the state of a search index'
            cand backfill 'Backfill a search index in batches'
            cand drop 'Drop a search index'
        }
        &'qp;search-index;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'kind'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;search-index;backfill'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'kind'
            cand --batch-size 'batch-size'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;search-index;drop'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'kind'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;doctor'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand labels 'Report graph-label drift and recommended floors'
        }
        &'qp;doctor;labels'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;pack'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verify 'verify'
            cand --out 'out'
            cand --space 'space'
            cand --name 'name'
            cand --version 'version'
            cand --shapes 'shapes'
            cand --queries 'queries'
            cand --repo 'repo'
            cand --repo-sha 'repo-sha'
            cand --model-id 'model-id'
            cand --model-version 'model-version'
            cand --destination 'destination'
            cand --format 'format'
            cand --with-vectors 'with-vectors'
            cand --full 'full'
            cand --allow-missing-embedding-recipe 'allow-missing-embedding-recipe'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;unpack'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --into 'into'
            cand --expect-repo 'expect-repo'
            cand --head-sha 'head-sha'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;restore'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --force 'force'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;share'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --output 'output'
            cand --graph 'graph'
            cand --group-id 'group-id'
            cand --construct 'construct'
            cand --shapes 'shapes'
            cand --queries 'queries'
            cand --alignments 'alignments'
            cand --parent-share 'parent-share'
            cand --since 'since'
            cand --destination 'destination'
            cand --pack-dir 'pack-dir'
            cand --actor 'actor'
            cand --attest-key 'attest-key'
            cand --attest-issued-at 'attest-issued-at'
            cand --attest-ttl 'attest-ttl'
            cand --attest-agent 'attest-agent'
            cand --attest-session 'attest-session'
            cand --attest-introducer 'attest-introducer'
            cand --attest-nonce 'attest-nonce'
            cand --pointers 'pointers'
            cand --no-shapes 'no-shapes'
            cand --no-queries 'no-queries'
            cand --turtle 'turtle'
            cand --attest 'attest'
            cand --project 'project'
            cand -h 'Print help'
            cand --help 'Print help'
            cand diff 'Diff two packs by entity, or write a PR-review report'
        }
        &'qp;share;diff'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --format 'format'
            cand --old-shapes 'old-shapes'
            cand --new-shapes 'new-shapes'
            cand --decisions 'decisions'
            cand --report 'report'
            cand --fail-on-introduced 'fail-on-introduced'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;merge'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'actor'
            cand --emit-decisions 'emit-decisions'
            cand --decisions 'decisions'
            cand --reviewer 'reviewer'
            cand --slot-scoped 'slot-scoped'
            cand --propose 'propose'
            cand --dry-run 'dry-run'
            cand -h 'Print help'
            cand --help 'Print help'
            cand show 'Show a merge''s conflicts in the standard layout'
            cand resolve 'Record one choice on one merge conflict'
        }
        &'qp;merge;show'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --format 'format'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;merge;resolve'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --choose 'choose'
            cand --value 'value'
            cand --by 'by'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;import'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --source 'source'
            cand --actor 'actor'
            cand --destination 'destination'
            cand --query-namespace 'query-namespace'
            cand --replace-queries 'replace-queries'
            cand -h 'Print help'
            cand --help 'Print help'
            cand delta 'Verify a delta share against its parent share'
            cand promote 'Promote a staged share into the store'
            cand review 'Review staged shares awaiting promotion'
        }
        &'qp;import;delta'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'actor'
            cand --destination 'destination'
            cand --query-namespace 'query-namespace'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;import;promote'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'actor'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;import;review'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand pending 'List staged shares awaiting review'
            cand notify 'Report staged shares older than an age threshold to a route'
            cand rejected 'Mark a staged share rejected, with a reason'
            cand expired 'Mark a staged share expired, with a reason'
            cand reopen 'Reopen a rejected or expired staged share'
        }
        &'qp;import;review;pending'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --after 'after'
            cand --limit 'limit'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;import;review;notify'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --after 'after'
            cand --limit 'limit'
            cand --age-seconds 'age-seconds'
            cand --route 'route'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;import;review;rejected'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'actor'
            cand --reason 'reason'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;import;review;expired'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'actor'
            cand --reason 'reason'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;import;review;reopen'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'actor'
            cand --reason 'reason'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;compose'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --repo 'repo'
            cand --shapes-from 'shapes-from'
            cand --actor 'actor'
            cand --destination 'destination'
            cand --revoke-tombstone 'revoke-tombstone'
            cand --pack-key 'pack-key'
            cand --reason 'reason'
            cand --use-pointer 'use-pointer'
            cand --declaration 'declaration'
            cand --authority 'authority'
            cand --repo-remote 'repo-remote'
            cand --pointer-cache 'pointer-cache'
            cand --rollback 'rollback'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;changes'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --from 'from'
            cand --capture 'capture'
            cand --limit 'limit'
            cand --graph 'graph'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;follow'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --primary 'primary'
            cand --page-txs 'page-txs'
            cand --page-gap-ms 'page-gap-ms'
            cand --bootstrap-only 'bootstrap-only'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;acquire'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand scope 'Report the bounded row and byte scope of a direct-main acquisition'
        }
        &'qp;acquire;scope'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --source-store-id 'source-store-id'
            cand --graph 'graph'
            cand --tx 'tx'
            cand --max-vm-steps 'max-vm-steps'
            cand --deadline-ms 'deadline-ms'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;migrate-vectors'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --from 'from'
            cand --to 'to'
            cand --dry-run 'dry-run'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;git-merge'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand show 'Show a qpack''s pending merge decisions in the standard layout'
            cand resolve 'Record one choice in the Git merge in progress and rewrite the qpack'
        }
        &'qp;git-merge;show'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --format 'format'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;git-merge;resolve'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --choose 'choose'
            cand --by 'by'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;merge-driver'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;pendant-resolve'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;pendant-check'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --history 'history'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;diff-textconv'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;mcp'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --mcp-token-file 'mcp-token-file'
            cand --bind 'bind'
            cand --primary-url 'primary-url'
            cand --read-only 'read-only'
            cand --embed-backfill 'embed-backfill'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;hook'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand session-capture 'Stop hook: solicit one knowledge episode per session from stdin JSON'
        }
        &'qp;hook;session-capture'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;hooks'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand bundle 'Print quipu''s hook bundle (st.hook-bundle/1)'
            cand install 'Install quipu''s hooks into Claude Code or Codex'
            cand uninstall 'Remove quipu''s hooks from Claude Code or Codex'
            cand status 'Report whether quipu''s hooks are installed'
        }
        &'qp;hooks;bundle'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;hooks;install'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --harness 'harness'
            cand --project 'project'
            cand --no-st 'no-st'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;hooks;uninstall'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --harness 'harness'
            cand --project 'project'
            cand --no-st 'no-st'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;hooks;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --harness 'harness'
            cand --project 'project'
            cand --no-st 'no-st'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;policy'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand draft 'Draft advisory policy Turtle from an exemplar and an intent'
            cand backtest 'Backtest a candidate policy over recorded transactions'
        }
        &'qp;policy;draft'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --exemplar 'exemplar'
            cand --name 'name'
            cand --label 'label'
            cand --targets 'targets'
            cand --claim 'claim'
            cand --class 'class'
            cand --point 'point'
            cand --layer 'layer'
            cand --authority 'authority'
            cand --out 'out'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;policy;backtest'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --last-txs 'last-txs'
            cand --from-tx 'from-tx'
            cand --to-tx 'to-tx'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;gate'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand shadow 'Judge a candidate policy set over recorded history on a quiescent copy'
        }
        &'qp;gate;shadow'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --rules 'rules'
            cand --since 'since'
            cand --from-tx 'from-tx'
            cand --to-tx 'to-tx'
            cand --last-txs 'last-txs'
            cand --mode 'mode'
            cand --max-txs 'max-txs'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;path'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand cone 'Compute the provenance cone of a trajectory'
            cand backtest 'Backtest a pruned trajectory over recorded history'
            cand draft 'Draft a golden-path policy from a trajectory as Turtle for review'
        }
        &'qp;path;cone'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --via 'via'
            cand --hops 'hops'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;path;backtest'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --omit 'omit'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;path;draft'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --name 'name'
            cand --label 'label'
            cand --via 'via'
            cand --omit 'omit'
            cand --by 'by'
            cand --dead-end 'dead-end'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;align'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand propose 'Propose scored alignments between two named graphs as an SSSOM set'
            cand decide 'Apply reviewer accept and negate decisions to a mapping set'
            cand apply 'Materialize decided alignments into a derived alignment graph'
        }
        &'qp;align;propose'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --set-id 'set-id'
            cand --out 'out'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;align;decide'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --decisions 'decisions'
            cand --reviewer 'reviewer'
            cand --out 'out'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;align;apply'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph-a 'graph-a'
            cand --graph-b 'graph-b'
            cand --expected-version 'expected-version'
            cand --actor 'actor'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;propose'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --status 'status'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List schema change proposals, optionally by status'
            cand submit 'Submit a schema change proposal'
            cand plan 'Report a pending proposal''s blast radius and 0-100 risk score without writing; exit 0 pass, 1 over threshold, 2 not determined'
            cand accept 'Accept a pending proposal; refused when its plan is over the risk threshold unless --accept-risk --reason'
            cand reject 'Reject a pending proposal'
        }
        &'qp;propose;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --status 'status'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;propose;submit'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --proposer 'proposer'
            cand --rationale 'rationale'
            cand --trigger 'trigger'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;propose;plan'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --format 'format'
            cand --risk-threshold 'risk-threshold'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;propose;accept'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --decided-by 'decided-by'
            cand --note 'note'
            cand --reason 'reason'
            cand --risk-threshold 'risk-threshold'
            cand --accept-risk 'accept-risk'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;propose;reject'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --decided-by 'decided-by'
            cand --note 'note'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;memory'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand read 'Read a versioned memory file for an owner'
            cand export 'Export an owner''s memory files as a file-data envelope'
            cand receipt 'Compute the receipt a memory commit request would produce'
            cand commit 'Commit a versioned memory file change'
        }
        &'qp;memory;read'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;memory;export'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;memory;receipt'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;memory;commit'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand --local-authority 'local-authority'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;attest'= {
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
        &'qp;attest;register'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --agent 'agent'
            cand --session 'session'
            cand --public-key 'public-key'
            cand --introducer 'introducer'
            cand --issued-at 'issued-at'
            cand --expires-at 'expires-at'
            cand --allow-write 'allow-write'
            cand --require-v2 'require-v2'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;attest;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;attest;allow-write'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;attest;deny-write'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;attest;require-v2'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;attest;allow-v1'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;attest;revoke'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;signer'= {
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
        &'qp;signer;enroll'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --credential-id 'credential-id'
            cand --cose-key 'cose-key'
            cand --scheme 'scheme'
            cand --origin 'origin'
            cand --rp-id 'rp-id'
            cand --actor 'actor'
            cand --graph 'graph'
            cand --expires-at 'expires-at'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;signer;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;signer;enable'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;signer;disable'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;signer;revoke'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;signer;protect-graph'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;deck'= {
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
        &'qp;deck;configure'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --origin 'origin'
            cand --rp-id 'rp-id'
            cand --actor 'actor'
            cand --author 'author'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;deck;enroll'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --credential-id 'credential-id'
            cand --cose-key 'cose-key'
            cand --scheme 'scheme'
            cand --expires-at 'expires-at'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;deck;enable'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;deck;disable'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;deck;revoke'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;deck;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;trust-root'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand challenge 'Issue the proof-of-possession challenge for a first verifier key'
            cand bootstrap 'Enroll the first verifier key with its signed challenge'
            cand status 'Show the root-of-trust state'
        }
        &'qp;trust-root;challenge'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verifier 'verifier'
            cand --public-key 'public-key'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;trust-root;bootstrap'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verifier 'verifier'
            cand --public-key 'public-key'
            cand --pop-signature 'pop-signature'
            cand --attests 'attests'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;trust-root;status'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph'= {
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
        &'qp;graph;create'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'actor'
            cand --timestamp 'timestamp'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph;label'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --label 'label'
            cand --actor 'actor'
            cand --timestamp 'timestamp'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --kind 'kind'
            cand --graph 'graph'
            cand --frozen 'frozen'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph;import'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --as 'as'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph;freeze'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --out 'out'
            cand --actor 'actor'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph;thaw'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --actor 'actor'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph;overlay-create'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --input 'input'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph;overlay-write'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --input 'input'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;graph;overlay-compose'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --input 'input'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;db'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand respace 'Copy the store into a term space'
            cand attach 'List the databases mounted alongside this store'
            cand migrate-inferred 'Move engine-derived facts into their companion inferred graphs'
        }
        &'qp;db;respace'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --into 'into'
            cand --out 'out'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;db;attach'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --list 'list'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;db;migrate-inferred'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;events'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
            cand refusals 'List recorded write refusals'
        }
        &'qp;events;refusals'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;fork'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --name 'name'
            cand --timestamp 'timestamp'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List named forks'
            cand diff 'Diff two forks'
            cand drop 'Drop a named fork'
            cand promote 'Promote a fork to ROOT, with reviewable conflict decisions'
        }
        &'qp;fork;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;fork;diff'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;fork;drop'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;fork;promote'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --timestamp 'timestamp'
            cand --emit-decisions 'emit-decisions'
            cand --decisions 'decisions'
            cand --reviewer 'reviewer'
            cand --actor 'actor'
            cand --propose 'propose'
            cand --dry-run 'dry-run'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;audit'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --repo 'repo'
            cand --from 'from'
            cand --to 'to'
            cand --yupana 'yupana'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
            cand inventory 'List the policies the audit checks against'
            cand namespace 'Report base-namespace drift'
            cand replay 'Re-derive a gate decision as of its transaction'
            cand tree 'Reassemble the dispatch forest from a trace''s principal chains'
            cand inheritance 'Check whether inherited constraints stopped applying under delegation'
            cand quarantine 'List refused-attempt evidence behind verdicts'
        }
        &'qp;audit;inventory'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;audit;namespace'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;audit;replay'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --delta 'delta'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;audit;tree'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;audit;inheritance'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;audit;quarantine'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verdict 'verdict'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
            cand list 'List refused-attempt evidence, optionally for one verdict'
            cand purge 'Purge quarantined evidence by graph, age or all'
        }
        &'qp;audit;quarantine;list'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --verdict 'verdict'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
        &'qp;audit;quarantine;purge'= {
            cand --db 'Store file (default: .bobbin/quipu/quipu.db; configuration may override it)'
            cand --graph 'graph'
            cand --before 'before'
            cand --older-than 'older-than'
            cand --all 'all'
            cand --json 'json'
            cand -h 'Print help'
            cand --help 'Print help'
        }
    ]
    $completions[$command]
}
