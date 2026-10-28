"""Scoring for the agent-branches benchmark, in plain values.

A port of examples/agentbranches/score.rs (intents, silent losses, maxCount
violations) and of the merge paper's oracle (examples/mergebench/generate.rs
`ground_truth`, without the alias class, which no arm here can see). run.py
re-scores every Rust arm with this module and refuses to publish if the two
scorers disagree, so the TerminusDB arm is scored by the same rules.

Slots are (entity, predicate) tuples, values are plain strings: bench-namespace
IRIs without the namespace, literals without quotes.
"""


def bounds(scenario):
    """Predicate -> sh:maxCount, from the scenario's own shapes."""
    out = {p: 1 for p in scenario['functional']}
    out.update({p: n for p, n in scenario['bounded'].items() if n is not None})
    return out


def slots_of_docs(docs):
    """{entity: {predicate: [values]}} -> {(entity, predicate): frozenset}."""
    out = {}
    for e, fields in docs.items():
        for p, values in fields.items():
            if values:
                out[(e, p)] = frozenset(values)
    return out


def set_three_way(b, o, t):
    return frozenset((b & o & t) | (o - b) | (t - b))


def needed(base, ours, theirs, bound):
    """Slots a person must decide when merging theirs into ours over base."""
    out = set()
    empty = frozenset()
    for slot in set(base) | set(ours) | set(theirs):
        n = bound.get(slot[1])
        if n is None:
            continue
        bv, ov, tv = base.get(slot, empty), ours.get(slot, empty), theirs.get(slot, empty)
        if ov == tv:
            continue
        if len(set_three_way(bv, ov, tv)) > n:
            out.add(slot)
            continue
        if n == 1 and bv:
            ours_removed, theirs_removed = not ov, not tv
            ours_changed = bool(ov) and ov != bv
            theirs_changed = bool(tv) and tv != bv
            if (ours_removed and theirs_changed) or (theirs_removed and ours_changed):
                out.add(slot)
    return out


def intents(scenario):
    """Every scripted write's intent: ('is', v), ('empty',) or ('has', v)."""
    last = {}
    adds = set()
    ops = list(scenario['root']) + [op for ops in scenario['agents'] for op in ops]
    for op in ops:
        slot = (op['entity'], op['predicate'])
        if op['kind'] == 'add':
            adds.add((op['writer'], slot, ('has', op['value'])))
        elif op['kind'] in ('set', 'correct'):
            last[(op['writer'], slot)] = ('is', op['value'])
        else:
            last[(op['writer'], slot)] = ('empty',)
    return [(w, s, i) for (w, s), i in last.items()] + sorted(adds)


def reflected(intent, values):
    if intent[0] == 'is':
        return values == frozenset([intent[1]])
    if intent[0] == 'empty':
        return not values
    return intent[1] in values


def silently_lost(writer, slot, intent, states, asked_by_promotion):
    """A write is due in ROOT from the state its writer's promotion produced
    (the live writer's, from before the first promotion). The first later
    state that does not reflect it is where it was dropped; the drop is silent
    unless the promotion that produced that state asked about the slot."""
    due = int(writer.split('-', 1)[1]) + 1 if writer.startswith('agent-') else 0
    empty = frozenset()
    for k in range(due, len(states)):
        if not reflected(intent, states[k].get(slot, empty)):
            return k == 0 or slot not in asked_by_promotion[k - 1]
    return False


def silent_losses(scenario, states, asked_by_promotion):
    """Writes dropped from ROOT at a promotion that did not ask about the slot."""
    return sum(1 for w, slot, intent in intents(scenario)
               if silently_lost(w, slot, intent, states, asked_by_promotion))


def violations(final, bound):
    return sum(1 for slot, vs in final.items()
               if bound.get(slot[1]) is not None and len(vs) > bound[slot[1]])


def parse_slot_key(key):
    e, p = key.split('|', 1)
    return (e, p)


def rescore_rust_arm(scenario, arm):
    """Silent losses and violations of a Rust-scored arm, recomputed here from
    its emitted states (deltas over base) and per-promotion questions."""
    base = slots_of_docs(scenario['base'])
    states = []
    for delta in arm['states']:
        st = dict(base)
        for k, v in delta.items():
            slot = parse_slot_key(k)
            if v:
                st[slot] = frozenset(v)
            else:
                st.pop(slot, None)
        states.append(st)
    asked = [{parse_slot_key(k) for k in p} for p in arm['asked_by_promotion']]
    return silent_losses(scenario, states, asked), violations(states[-1], bounds(scenario))
