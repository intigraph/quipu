_quipu() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="quipu"
                ;;
            quipu,acquire)
                cmd="quipu__subcmd__acquire"
                ;;
            quipu,align)
                cmd="quipu__subcmd__align"
                ;;
            quipu,attest)
                cmd="quipu__subcmd__attest"
                ;;
            quipu,audit)
                cmd="quipu__subcmd__audit"
                ;;
            quipu,changes)
                cmd="quipu__subcmd__changes"
                ;;
            quipu,compose)
                cmd="quipu__subcmd__compose"
                ;;
            quipu,cord)
                cmd="quipu__subcmd__cord"
                ;;
            quipu,db)
                cmd="quipu__subcmd__db"
                ;;
            quipu,deck)
                cmd="quipu__subcmd__deck"
                ;;
            quipu,demotions)
                cmd="quipu__subcmd__demotions"
                ;;
            quipu,diff-textconv)
                cmd="quipu__subcmd__diff__subcmd__textconv"
                ;;
            quipu,doctor)
                cmd="quipu__subcmd__doctor"
                ;;
            quipu,entity-ref)
                cmd="quipu__subcmd__entity__subcmd__ref"
                ;;
            quipu,episode)
                cmd="quipu__subcmd__episode"
                ;;
            quipu,events)
                cmd="quipu__subcmd__events"
                ;;
            quipu,explain)
                cmd="quipu__subcmd__explain"
                ;;
            quipu,export)
                cmd="quipu__subcmd__export"
                ;;
            quipu,follow)
                cmd="quipu__subcmd__follow"
                ;;
            quipu,fork)
                cmd="quipu__subcmd__fork"
                ;;
            quipu,gate)
                cmd="quipu__subcmd__gate"
                ;;
            quipu,git-merge)
                cmd="quipu__subcmd__git__subcmd__merge"
                ;;
            quipu,graph)
                cmd="quipu__subcmd__graph"
                ;;
            quipu,hook)
                cmd="quipu__subcmd__hook"
                ;;
            quipu,hooks)
                cmd="quipu__subcmd__hooks"
                ;;
            quipu,impact)
                cmd="quipu__subcmd__impact"
                ;;
            quipu,import)
                cmd="quipu__subcmd__import"
                ;;
            quipu,ingest)
                cmd="quipu__subcmd__ingest"
                ;;
            quipu,install-alias)
                cmd="quipu__subcmd__install__subcmd__alias"
                ;;
            quipu,knot)
                cmd="quipu__subcmd__knot"
                ;;
            quipu,load)
                cmd="quipu__subcmd__load"
                ;;
            quipu,mcp)
                cmd="quipu__subcmd__mcp"
                ;;
            quipu,memory)
                cmd="quipu__subcmd__memory"
                ;;
            quipu,merge)
                cmd="quipu__subcmd__merge"
                ;;
            quipu,merge-driver)
                cmd="quipu__subcmd__merge__subcmd__driver"
                ;;
            quipu,migrate-vectors)
                cmd="quipu__subcmd__migrate__subcmd__vectors"
                ;;
            quipu,namespaces)
                cmd="quipu__subcmd__namespaces"
                ;;
            quipu,ontology)
                cmd="quipu__subcmd__ontology"
                ;;
            quipu,pack)
                cmd="quipu__subcmd__pack"
                ;;
            quipu,path)
                cmd="quipu__subcmd__path"
                ;;
            quipu,pendant-check)
                cmd="quipu__subcmd__pendant__subcmd__check"
                ;;
            quipu,pendant-resolve)
                cmd="quipu__subcmd__pendant__subcmd__resolve"
                ;;
            quipu,policy)
                cmd="quipu__subcmd__policy"
                ;;
            quipu,prime)
                cmd="quipu__subcmd__prime"
                ;;
            quipu,project)
                cmd="quipu__subcmd__project"
                ;;
            quipu,propose)
                cmd="quipu__subcmd__propose"
                ;;
            quipu,query)
                cmd="quipu__subcmd__query"
                ;;
            quipu,read)
                cmd="quipu__subcmd__read"
                ;;
            quipu,reason)
                cmd="quipu__subcmd__reason"
                ;;
            quipu,render)
                cmd="quipu__subcmd__render"
                ;;
            quipu,repl)
                cmd="quipu__subcmd__repl"
                ;;
            quipu,report)
                cmd="quipu__subcmd__report"
                ;;
            quipu,restore)
                cmd="quipu__subcmd__restore"
                ;;
            quipu,retract)
                cmd="quipu__subcmd__retract"
                ;;
            quipu,rule-query)
                cmd="quipu__subcmd__rule__subcmd__query"
                ;;
            quipu,search)
                cmd="quipu__subcmd__search"
                ;;
            quipu,search-index)
                cmd="quipu__subcmd__search__subcmd__index"
                ;;
            quipu,shapes)
                cmd="quipu__subcmd__shapes"
                ;;
            quipu,share)
                cmd="quipu__subcmd__share"
                ;;
            quipu,signer)
                cmd="quipu__subcmd__signer"
                ;;
            quipu,stats)
                cmd="quipu__subcmd__stats"
                ;;
            quipu,status)
                cmd="quipu__subcmd__status"
                ;;
            quipu,trust-root)
                cmd="quipu__subcmd__trust__subcmd__root"
                ;;
            quipu,unpack)
                cmd="quipu__subcmd__unpack"
                ;;
            quipu,unravel)
                cmd="quipu__subcmd__unravel"
                ;;
            quipu,validate)
                cmd="quipu__subcmd__validate"
                ;;
            quipu,version)
                cmd="quipu__subcmd__version"
                ;;
            quipu__subcmd__acquire,scope)
                cmd="quipu__subcmd__acquire__subcmd__scope"
                ;;
            quipu__subcmd__align,apply)
                cmd="quipu__subcmd__align__subcmd__apply"
                ;;
            quipu__subcmd__align,decide)
                cmd="quipu__subcmd__align__subcmd__decide"
                ;;
            quipu__subcmd__align,propose)
                cmd="quipu__subcmd__align__subcmd__propose"
                ;;
            quipu__subcmd__attest,allow-v1)
                cmd="quipu__subcmd__attest__subcmd__allow__subcmd__v1"
                ;;
            quipu__subcmd__attest,allow-write)
                cmd="quipu__subcmd__attest__subcmd__allow__subcmd__write"
                ;;
            quipu__subcmd__attest,deny-write)
                cmd="quipu__subcmd__attest__subcmd__deny__subcmd__write"
                ;;
            quipu__subcmd__attest,list)
                cmd="quipu__subcmd__attest__subcmd__list"
                ;;
            quipu__subcmd__attest,register)
                cmd="quipu__subcmd__attest__subcmd__register"
                ;;
            quipu__subcmd__attest,require-v2)
                cmd="quipu__subcmd__attest__subcmd__require__subcmd__v2"
                ;;
            quipu__subcmd__attest,revoke)
                cmd="quipu__subcmd__attest__subcmd__revoke"
                ;;
            quipu__subcmd__audit,inheritance)
                cmd="quipu__subcmd__audit__subcmd__inheritance"
                ;;
            quipu__subcmd__audit,inventory)
                cmd="quipu__subcmd__audit__subcmd__inventory"
                ;;
            quipu__subcmd__audit,namespace)
                cmd="quipu__subcmd__audit__subcmd__namespace"
                ;;
            quipu__subcmd__audit,quarantine)
                cmd="quipu__subcmd__audit__subcmd__quarantine"
                ;;
            quipu__subcmd__audit,replay)
                cmd="quipu__subcmd__audit__subcmd__replay"
                ;;
            quipu__subcmd__audit,tree)
                cmd="quipu__subcmd__audit__subcmd__tree"
                ;;
            quipu__subcmd__audit__subcmd__quarantine,list)
                cmd="quipu__subcmd__audit__subcmd__quarantine__subcmd__list"
                ;;
            quipu__subcmd__audit__subcmd__quarantine,purge)
                cmd="quipu__subcmd__audit__subcmd__quarantine__subcmd__purge"
                ;;
            quipu__subcmd__db,attach)
                cmd="quipu__subcmd__db__subcmd__attach"
                ;;
            quipu__subcmd__db,migrate-inferred)
                cmd="quipu__subcmd__db__subcmd__migrate__subcmd__inferred"
                ;;
            quipu__subcmd__db,respace)
                cmd="quipu__subcmd__db__subcmd__respace"
                ;;
            quipu__subcmd__deck,configure)
                cmd="quipu__subcmd__deck__subcmd__configure"
                ;;
            quipu__subcmd__deck,disable)
                cmd="quipu__subcmd__deck__subcmd__disable"
                ;;
            quipu__subcmd__deck,enable)
                cmd="quipu__subcmd__deck__subcmd__enable"
                ;;
            quipu__subcmd__deck,enroll)
                cmd="quipu__subcmd__deck__subcmd__enroll"
                ;;
            quipu__subcmd__deck,revoke)
                cmd="quipu__subcmd__deck__subcmd__revoke"
                ;;
            quipu__subcmd__deck,status)
                cmd="quipu__subcmd__deck__subcmd__status"
                ;;
            quipu__subcmd__doctor,labels)
                cmd="quipu__subcmd__doctor__subcmd__labels"
                ;;
            quipu__subcmd__entity__subcmd__ref,mint)
                cmd="quipu__subcmd__entity__subcmd__ref__subcmd__mint"
                ;;
            quipu__subcmd__entity__subcmd__ref,resolve)
                cmd="quipu__subcmd__entity__subcmd__ref__subcmd__resolve"
                ;;
            quipu__subcmd__events,refusals)
                cmd="quipu__subcmd__events__subcmd__refusals"
                ;;
            quipu__subcmd__fork,diff)
                cmd="quipu__subcmd__fork__subcmd__diff"
                ;;
            quipu__subcmd__fork,drop)
                cmd="quipu__subcmd__fork__subcmd__drop"
                ;;
            quipu__subcmd__fork,list)
                cmd="quipu__subcmd__fork__subcmd__list"
                ;;
            quipu__subcmd__fork,promote)
                cmd="quipu__subcmd__fork__subcmd__promote"
                ;;
            quipu__subcmd__gate,shadow)
                cmd="quipu__subcmd__gate__subcmd__shadow"
                ;;
            quipu__subcmd__git__subcmd__merge,resolve)
                cmd="quipu__subcmd__git__subcmd__merge__subcmd__resolve"
                ;;
            quipu__subcmd__git__subcmd__merge,show)
                cmd="quipu__subcmd__git__subcmd__merge__subcmd__show"
                ;;
            quipu__subcmd__graph,create)
                cmd="quipu__subcmd__graph__subcmd__create"
                ;;
            quipu__subcmd__graph,freeze)
                cmd="quipu__subcmd__graph__subcmd__freeze"
                ;;
            quipu__subcmd__graph,import)
                cmd="quipu__subcmd__graph__subcmd__import"
                ;;
            quipu__subcmd__graph,label)
                cmd="quipu__subcmd__graph__subcmd__label"
                ;;
            quipu__subcmd__graph,list)
                cmd="quipu__subcmd__graph__subcmd__list"
                ;;
            quipu__subcmd__graph,overlay-compose)
                cmd="quipu__subcmd__graph__subcmd__overlay__subcmd__compose"
                ;;
            quipu__subcmd__graph,overlay-create)
                cmd="quipu__subcmd__graph__subcmd__overlay__subcmd__create"
                ;;
            quipu__subcmd__graph,overlay-write)
                cmd="quipu__subcmd__graph__subcmd__overlay__subcmd__write"
                ;;
            quipu__subcmd__graph,thaw)
                cmd="quipu__subcmd__graph__subcmd__thaw"
                ;;
            quipu__subcmd__hook,session-capture)
                cmd="quipu__subcmd__hook__subcmd__session__subcmd__capture"
                ;;
            quipu__subcmd__hooks,bundle)
                cmd="quipu__subcmd__hooks__subcmd__bundle"
                ;;
            quipu__subcmd__hooks,install)
                cmd="quipu__subcmd__hooks__subcmd__install"
                ;;
            quipu__subcmd__hooks,status)
                cmd="quipu__subcmd__hooks__subcmd__status"
                ;;
            quipu__subcmd__hooks,uninstall)
                cmd="quipu__subcmd__hooks__subcmd__uninstall"
                ;;
            quipu__subcmd__import,delta)
                cmd="quipu__subcmd__import__subcmd__delta"
                ;;
            quipu__subcmd__import,promote)
                cmd="quipu__subcmd__import__subcmd__promote"
                ;;
            quipu__subcmd__import,review)
                cmd="quipu__subcmd__import__subcmd__review"
                ;;
            quipu__subcmd__import__subcmd__review,expired)
                cmd="quipu__subcmd__import__subcmd__review__subcmd__expired"
                ;;
            quipu__subcmd__import__subcmd__review,notify)
                cmd="quipu__subcmd__import__subcmd__review__subcmd__notify"
                ;;
            quipu__subcmd__import__subcmd__review,pending)
                cmd="quipu__subcmd__import__subcmd__review__subcmd__pending"
                ;;
            quipu__subcmd__import__subcmd__review,rejected)
                cmd="quipu__subcmd__import__subcmd__review__subcmd__rejected"
                ;;
            quipu__subcmd__import__subcmd__review,reopen)
                cmd="quipu__subcmd__import__subcmd__review__subcmd__reopen"
                ;;
            quipu__subcmd__memory,commit)
                cmd="quipu__subcmd__memory__subcmd__commit"
                ;;
            quipu__subcmd__memory,export)
                cmd="quipu__subcmd__memory__subcmd__export"
                ;;
            quipu__subcmd__memory,read)
                cmd="quipu__subcmd__memory__subcmd__read"
                ;;
            quipu__subcmd__memory,receipt)
                cmd="quipu__subcmd__memory__subcmd__receipt"
                ;;
            quipu__subcmd__merge,resolve)
                cmd="quipu__subcmd__merge__subcmd__resolve"
                ;;
            quipu__subcmd__merge,show)
                cmd="quipu__subcmd__merge__subcmd__show"
                ;;
            quipu__subcmd__namespaces,apply)
                cmd="quipu__subcmd__namespaces__subcmd__apply"
                ;;
            quipu__subcmd__namespaces,list)
                cmd="quipu__subcmd__namespaces__subcmd__list"
                ;;
            quipu__subcmd__ontology,list)
                cmd="quipu__subcmd__ontology__subcmd__list"
                ;;
            quipu__subcmd__ontology,load)
                cmd="quipu__subcmd__ontology__subcmd__load"
                ;;
            quipu__subcmd__ontology,remove)
                cmd="quipu__subcmd__ontology__subcmd__remove"
                ;;
            quipu__subcmd__path,backtest)
                cmd="quipu__subcmd__path__subcmd__backtest"
                ;;
            quipu__subcmd__path,cone)
                cmd="quipu__subcmd__path__subcmd__cone"
                ;;
            quipu__subcmd__path,draft)
                cmd="quipu__subcmd__path__subcmd__draft"
                ;;
            quipu__subcmd__policy,backtest)
                cmd="quipu__subcmd__policy__subcmd__backtest"
                ;;
            quipu__subcmd__policy,draft)
                cmd="quipu__subcmd__policy__subcmd__draft"
                ;;
            quipu__subcmd__propose,accept)
                cmd="quipu__subcmd__propose__subcmd__accept"
                ;;
            quipu__subcmd__propose,list)
                cmd="quipu__subcmd__propose__subcmd__list"
                ;;
            quipu__subcmd__propose,reject)
                cmd="quipu__subcmd__propose__subcmd__reject"
                ;;
            quipu__subcmd__propose,submit)
                cmd="quipu__subcmd__propose__subcmd__submit"
                ;;
            quipu__subcmd__search__subcmd__index,backfill)
                cmd="quipu__subcmd__search__subcmd__index__subcmd__backfill"
                ;;
            quipu__subcmd__search__subcmd__index,drop)
                cmd="quipu__subcmd__search__subcmd__index__subcmd__drop"
                ;;
            quipu__subcmd__search__subcmd__index,status)
                cmd="quipu__subcmd__search__subcmd__index__subcmd__status"
                ;;
            quipu__subcmd__shapes,list)
                cmd="quipu__subcmd__shapes__subcmd__list"
                ;;
            quipu__subcmd__shapes,load)
                cmd="quipu__subcmd__shapes__subcmd__load"
                ;;
            quipu__subcmd__shapes,remove)
                cmd="quipu__subcmd__shapes__subcmd__remove"
                ;;
            quipu__subcmd__share,diff)
                cmd="quipu__subcmd__share__subcmd__diff"
                ;;
            quipu__subcmd__signer,disable)
                cmd="quipu__subcmd__signer__subcmd__disable"
                ;;
            quipu__subcmd__signer,enable)
                cmd="quipu__subcmd__signer__subcmd__enable"
                ;;
            quipu__subcmd__signer,enroll)
                cmd="quipu__subcmd__signer__subcmd__enroll"
                ;;
            quipu__subcmd__signer,protect-graph)
                cmd="quipu__subcmd__signer__subcmd__protect__subcmd__graph"
                ;;
            quipu__subcmd__signer,revoke)
                cmd="quipu__subcmd__signer__subcmd__revoke"
                ;;
            quipu__subcmd__signer,status)
                cmd="quipu__subcmd__signer__subcmd__status"
                ;;
            quipu__subcmd__trust__subcmd__root,bootstrap)
                cmd="quipu__subcmd__trust__subcmd__root__subcmd__bootstrap"
                ;;
            quipu__subcmd__trust__subcmd__root,challenge)
                cmd="quipu__subcmd__trust__subcmd__root__subcmd__challenge"
                ;;
            quipu__subcmd__trust__subcmd__root,status)
                cmd="quipu__subcmd__trust__subcmd__root__subcmd__status"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        quipu)
            opts="-h --db --help version install-alias knot load ingest read query rule-query cord unravel impact explain project report reason demotions episode retract shapes namespaces entity-ref ontology validate repl export render stats prime search search-index doctor pack unpack restore share status merge import compose changes follow acquire migrate-vectors git-merge merge-driver pendant-resolve pendant-check diff-textconv mcp hook hooks policy gate path align propose memory attest signer deck trust-root graph db events fork audit"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__acquire)
            opts="-h --db --help scope"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__acquire__subcmd__scope)
            opts="-h --db --source-store-id --graph --tx --max-vm-steps --deadline-ms --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --source-store-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tx)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --max-vm-steps)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --deadline-ms)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__align)
            opts="-h --db --help propose decide apply"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__align__subcmd__apply)
            opts="-h --db --graph-a --graph-b --expected-version --actor --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph-a)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph-b)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --expected-version)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__align__subcmd__decide)
            opts="-h --db --decisions --reviewer --out --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --decisions)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --reviewer)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --out)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__align__subcmd__propose)
            opts="-h --db --set-id --out --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --set-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --out)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__attest)
            opts="-h --db --help register list allow-write deny-write require-v2 allow-v1 revoke"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__attest__subcmd__allow__subcmd__v1)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__attest__subcmd__allow__subcmd__write)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__attest__subcmd__deny__subcmd__write)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__attest__subcmd__list)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__attest__subcmd__register)
            opts="-h --db --agent --session --public-key --introducer --issued-at --expires-at --allow-write --require-v2 --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --agent)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --session)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --public-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --introducer)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --issued-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --expires-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__attest__subcmd__require__subcmd__v2)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__attest__subcmd__revoke)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit)
            opts="-h --db --repo --from --to --yupana --json --help inventory namespace replay tree inheritance quarantine"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --repo)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --from)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --to)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --yupana)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit__subcmd__inheritance)
            opts="-h --db --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit__subcmd__inventory)
            opts="-h --db --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit__subcmd__namespace)
            opts="-h --db --graph --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit__subcmd__quarantine)
            opts="-h --db --verdict --json --help list purge"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --verdict)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit__subcmd__quarantine__subcmd__list)
            opts="-h --db --verdict --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --verdict)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit__subcmd__quarantine__subcmd__purge)
            opts="-h --db --graph --before --older-than --all --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --before)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --older-than)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit__subcmd__replay)
            opts="-h --db --delta --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --delta)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__audit__subcmd__tree)
            opts="-h --db --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__changes)
            opts="-h --db --from --capture --limit --graph --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --from)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --capture)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --limit)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__compose)
            opts="-h --db --repo --shapes-from --actor --destination --revoke-tombstone --pack-key --reason --use-pointer --declaration --authority --repo-remote --pointer-cache --rollback --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --repo)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --shapes-from)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --destination)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --revoke-tombstone)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --pack-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --reason)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --use-pointer)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --declaration)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --authority)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --repo-remote)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --pointer-cache)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__cord)
            opts="-h --db --type --limit --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --type)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --limit)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__db)
            opts="-h --db --help respace attach migrate-inferred"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__db__subcmd__attach)
            opts="-h --db --list --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__db__subcmd__migrate__subcmd__inferred)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__db__subcmd__respace)
            opts="-h --db --into --out --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --into)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --out)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__deck)
            opts="-h --db --help configure enroll enable disable revoke status"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__deck__subcmd__configure)
            opts="-h --db --origin --rp-id --actor --author --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --origin)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --rp-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --author)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__deck__subcmd__disable)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__deck__subcmd__enable)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__deck__subcmd__enroll)
            opts="-h --db --credential-id --cose-key --scheme --expires-at --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --credential-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --cose-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --scheme)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --expires-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__deck__subcmd__revoke)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__deck__subcmd__status)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__demotions)
            opts="-h --db --graph --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__diff__subcmd__textconv)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__doctor)
            opts="-h --db --help labels"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__doctor__subcmd__labels)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__entity__subcmd__ref)
            opts="-h --db --help mint resolve"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__entity__subcmd__ref__subcmd__mint)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__entity__subcmd__ref__subcmd__resolve)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__episode)
            opts="-h --db --base-ns --timestamp --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --base-ns)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__events)
            opts="-h --db --help refusals"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__events__subcmd__refusals)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__explain)
            opts="-h --db --depth --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --depth)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__export)
            opts="-h --db --graph --group-id --construct --format --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --group-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --construct)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --format)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__follow)
            opts="-h --db --primary --page-txs --page-gap-ms --bootstrap-only --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --primary)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --page-txs)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --page-gap-ms)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__fork)
            opts="-h --db --name --timestamp --help list diff drop promote"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --name)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__fork__subcmd__diff)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__fork__subcmd__drop)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__fork__subcmd__list)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__fork__subcmd__promote)
            opts="-h --db --timestamp --emit-decisions --decisions --reviewer --actor --propose --dry-run --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --emit-decisions)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --decisions)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --reviewer)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__gate)
            opts="-h --db --help shadow"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__gate__subcmd__shadow)
            opts="-h --db --rules --since --from-tx --to-tx --last-txs --mode --max-txs --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --rules)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --since)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --from-tx)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --to-tx)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --last-txs)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --mode)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --max-txs)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__git__subcmd__merge)
            opts="-h --db --help show resolve"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__git__subcmd__merge__subcmd__resolve)
            opts="-h --db --choose --by --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --choose)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --by)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__git__subcmd__merge__subcmd__show)
            opts="-h --db --format --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --format)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph)
            opts="-h --db --help create label list import freeze thaw overlay-create overlay-write overlay-compose"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__create)
            opts="-h --db --actor --timestamp --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__freeze)
            opts="-h --db --out --actor --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --out)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__import)
            opts="-h --db --as --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --as)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__label)
            opts="-h --db --label --actor --timestamp --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --label)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__list)
            opts="-h --db --kind --graph --frozen --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --kind)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__overlay__subcmd__compose)
            opts="-h --db --input --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --input)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__overlay__subcmd__create)
            opts="-h --db --input --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --input)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__overlay__subcmd__write)
            opts="-h --db --input --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --input)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__graph__subcmd__thaw)
            opts="-h --db --actor --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__hook)
            opts="-h --db --help session-capture"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__hook__subcmd__session__subcmd__capture)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__hooks)
            opts="-h --db --help bundle install uninstall status"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__hooks__subcmd__bundle)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__hooks__subcmd__install)
            opts="-h --db --harness --project --no-st --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --harness)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__hooks__subcmd__status)
            opts="-h --db --harness --project --no-st --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --harness)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__hooks__subcmd__uninstall)
            opts="-h --db --harness --project --no-st --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --harness)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__impact)
            opts="-h --db --hops --predicate --remove --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --hops)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --predicate)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import)
            opts="-h --db --source --actor --destination --query-namespace --replace-queries --help delta promote review"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --source)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --destination)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --query-namespace)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import__subcmd__delta)
            opts="-h --db --actor --destination --query-namespace --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --destination)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --query-namespace)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import__subcmd__promote)
            opts="-h --db --actor --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import__subcmd__review)
            opts="-h --db --help pending notify rejected expired reopen"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import__subcmd__review__subcmd__expired)
            opts="-h --db --actor --reason --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --reason)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import__subcmd__review__subcmd__notify)
            opts="-h --db --after --limit --age-seconds --route --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --after)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --limit)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --age-seconds)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --route)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import__subcmd__review__subcmd__pending)
            opts="-h --db --after --limit --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --after)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --limit)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import__subcmd__review__subcmd__rejected)
            opts="-h --db --actor --reason --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --reason)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__import__subcmd__review__subcmd__reopen)
            opts="-h --db --actor --reason --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --reason)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__ingest)
            opts="-h --db --graph --timestamp --declare-count --declare-sha256 --format --chunk --base-iri --actor --source --blank-node-scope --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --declare-count)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --declare-sha256)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --format)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --chunk)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --base-iri)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --source)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --blank-node-scope)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__install__subcmd__alias)
            opts="-h --db --bin-dir --check --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --bin-dir)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__knot)
            opts="-h --db --shapes --graph --timestamp --blank-node-scope --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --shapes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --blank-node-scope)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__load)
            opts="-h --db --shapes --graph --timestamp --blank-node-scope --destination --actor --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --shapes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --blank-node-scope)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --destination)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__mcp)
            opts="-h --db --mcp-token-file --bind --primary-url --read-only --embed-backfill --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --mcp-token-file)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --bind)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --primary-url)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__memory)
            opts="-h --db --help read export receipt commit"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__memory__subcmd__commit)
            opts="-h --db --graph --local-authority --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__memory__subcmd__export)
            opts="-h --db --graph --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__memory__subcmd__read)
            opts="-h --db --graph --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__memory__subcmd__receipt)
            opts="-h --db --graph --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__merge)
            opts="-h --db --actor --emit-decisions --decisions --reviewer --slot-scoped --propose --dry-run --help show resolve"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --emit-decisions)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --decisions)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --reviewer)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__merge__subcmd__driver)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__merge__subcmd__resolve)
            opts="-h --db --choose --value --by --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --choose)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --value)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --by)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__merge__subcmd__show)
            opts="-h --db --format --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --format)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__migrate__subcmd__vectors)
            opts="-h --db --from --to --dry-run --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --from)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --to)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__namespaces)
            opts="-h --db --help list apply"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__namespaces__subcmd__apply)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__namespaces__subcmd__list)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__ontology)
            opts="-h --db --help list load remove"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__ontology__subcmd__list)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__ontology__subcmd__load)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__ontology__subcmd__remove)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__pack)
            opts="-h --db --verify --out --space --name --version --shapes --queries --repo --repo-sha --model-id --model-version --destination --format --with-vectors --full --allow-missing-embedding-recipe --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --verify)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --out)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --space)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --name)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --version)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --shapes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --queries)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --repo)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --repo-sha)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --model-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --model-version)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --destination)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --format)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__path)
            opts="-h --db --help cone backtest draft"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__path__subcmd__backtest)
            opts="-h --db --omit --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --omit)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__path__subcmd__cone)
            opts="-h --db --via --hops --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --via)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --hops)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__path__subcmd__draft)
            opts="-h --db --name --label --via --omit --by --dead-end --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --name)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --label)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --via)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --omit)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --by)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --dead-end)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__pendant__subcmd__check)
            opts="-h --db --history --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__pendant__subcmd__resolve)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__policy)
            opts="-h --db --help draft backtest"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__policy__subcmd__backtest)
            opts="-h --db --last-txs --from-tx --to-tx --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --last-txs)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --from-tx)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --to-tx)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__policy__subcmd__draft)
            opts="-h --db --exemplar --name --label --targets --claim --class --point --layer --authority --out --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --exemplar)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --name)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --label)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --targets)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --claim)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --class)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --point)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --layer)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --authority)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --out)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__prime)
            opts="-h --db --section --brief --static --json --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --section)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__project)
            opts="-h --db --algorithm --type --predicate --graph --limit --damping --max-iters --tolerance --from --to --seed --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --algorithm)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --type)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --predicate)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --limit)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --damping)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --max-iters)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tolerance)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --from)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --to)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --seed)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__propose)
            opts="-h --db --status --help list submit accept reject"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --status)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__propose__subcmd__accept)
            opts="-h --db --decided-by --note --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --decided-by)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --note)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__propose__subcmd__list)
            opts="-h --db --status --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --status)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__propose__subcmd__reject)
            opts="-h --db --decided-by --note --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --decided-by)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --note)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__propose__subcmd__submit)
            opts="-h --db --proposer --rationale --trigger --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --proposer)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --rationale)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --trigger)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__query)
            opts="-h --db --valid-at --tx --fork --entailment --federation --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --valid-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tx)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --fork)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --entailment)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__read)
            opts="-h --db --valid-at --tx --fork --entailment --federation --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --valid-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tx)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --fork)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --entailment)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__reason)
            opts="-h --db --rules --reactive --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --rules)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__render)
            opts="-h --db --dest --graph --valid-at --server --query --template --layout --path --dry-run --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --dest)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --valid-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --server)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --query)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --template)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --layout)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__repl)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__report)
            opts="-h --db --type --predicate --hubs --surprises --questions --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --type)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --predicate)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --hubs)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --surprises)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --questions)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__restore)
            opts="-h --db --force --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__retract)
            opts="-h --db --predicate --timestamp --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --predicate)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --timestamp)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__rule__subcmd__query)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__search)
            opts="-h --db --mode --limit --alpha --rrf-k --decay --anchor --anchor-mode --direction --max-hops --via --fusion --valid-at --type --group --structured-query --filters --graph --graphs --explain --all-graphs --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --mode)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --limit)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --alpha)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --rrf-k)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --decay)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --anchor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --anchor-mode)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --direction)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --max-hops)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --via)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --fusion)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --valid-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --type)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --group)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --structured-query)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --filters)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graphs)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__search__subcmd__index)
            opts="-h --db --kind --batch-size --help status backfill drop"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --kind)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --batch-size)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__search__subcmd__index__subcmd__backfill)
            opts="-h --db --kind --batch-size --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --kind)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --batch-size)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__search__subcmd__index__subcmd__drop)
            opts="-h --db --kind --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --kind)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__search__subcmd__index__subcmd__status)
            opts="-h --db --kind --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --kind)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__shapes)
            opts="-h --db --help list load remove"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__shapes__subcmd__list)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__shapes__subcmd__load)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__shapes__subcmd__remove)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__share)
            opts="-h --db --output --graph --group-id --construct --shapes --queries --alignments --parent-share --since --destination --pack-dir --actor --attest-key --attest-issued-at --attest-ttl --attest-agent --attest-session --attest-introducer --attest-nonce --pointers --no-shapes --no-queries --turtle --attest --project --help diff"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --output)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --group-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --construct)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --shapes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --queries)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --alignments)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --parent-share)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --since)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --destination)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --pack-dir)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --attest-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --attest-issued-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --attest-ttl)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --attest-agent)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --attest-session)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --attest-introducer)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --attest-nonce)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --pointers)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__share__subcmd__diff)
            opts="-h --db --format --old-shapes --new-shapes --decisions --report --fail-on-introduced --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --format)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --old-shapes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --new-shapes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --decisions)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__signer)
            opts="-h --db --help enroll status enable disable revoke protect-graph"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__signer__subcmd__disable)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__signer__subcmd__enable)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__signer__subcmd__enroll)
            opts="-h --db --credential-id --cose-key --scheme --origin --rp-id --actor --graph --expires-at --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --credential-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --cose-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --scheme)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --origin)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --rp-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --actor)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --graph)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --expires-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__signer__subcmd__protect__subcmd__graph)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__signer__subcmd__revoke)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__signer__subcmd__status)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__stats)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__status)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__trust__subcmd__root)
            opts="-h --db --help challenge bootstrap status"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__trust__subcmd__root__subcmd__bootstrap)
            opts="-h --db --verifier --public-key --pop-signature --attests --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --verifier)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --public-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --pop-signature)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --attests)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__trust__subcmd__root__subcmd__challenge)
            opts="-h --db --verifier --public-key --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --verifier)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --public-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__trust__subcmd__root__subcmd__status)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__unpack)
            opts="-h --db --into --expect-repo --head-sha --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --into)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --expect-repo)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --head-sha)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__unravel)
            opts="-h --db --tx --valid-at --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tx)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --valid-at)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__validate)
            opts="-h --db --shapes --data --format --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --shapes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --data)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --format)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        quipu__subcmd__version)
            opts="-h --db --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --db)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _quipu -o nosort -o bashdefault -o default quipu
else
    complete -F _quipu -o bashdefault -o default quipu
fi
