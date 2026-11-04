<!-- markdownlint-disable MD029 -- question ids are file#number and must stay unique across sections (camayoc competency.py) -->

# Competency questions: merge behaviour

Questions the graph must answer after a merge, per use case of
[docs/design/merge-behaviour.md](../design/merge-behaviour.md) (aegis-karb.1).
Format: camayoc `scripts/competency.py` (a `##` section, numbered items,
`<parameters>` in angle brackets). None of them reads the shapes graph.

## Any merge

1. Which transactions did merge <merge-id> write, in order, and which of them
   received values and which decided them?
2. What was the value of <predicate> on <subject> at valid time <time>, as known
   at transaction <tx>?
3. Which values did merge <merge-id> receive and then reject, so that they hold
   no valid time?
4. Which slots did both sides change to the same value in merge <merge-id>, and
   what was decided for each?
5. Who decided the conflict on <predicate> of <subject> in merge <merge-id>, with
   which choice, and when?
6. Which ROOT values did merge <merge-id> close, and at what valid time?
7. Does any row hold a valid end before its valid start?
8. Does any single-valued slot hold two values with overlapping valid time?
9. How many slots did merge <merge-id> apply and how many did it hold?

## Share reconnect

10. Which share <share-id> and parent share did merge <merge-id> reconnect,
    and from which store?
11. Which values in ROOT came from share <share-id>, and from what authored time
    are they valid?
12. Do two peers that merged each other's shares hold the same value for
    <predicate> on <subject>?

## Fork promote

13. Which facts did promoting fork <fork> carry into ROOT, with their own valid
    times and original transactions?
14. When ROOT re-claimed a value after fork <fork> closed it, at what valid time
    did the promotion close ROOT's row?
15. Which slots of fork <fork> are still held, and to which transaction are
    their pending decision rows bound?

## Pendant merge

16. Which conflicts and convergent slots does the decisions file of pendant
    <pendant> record, and with which resolutions?
17. Does the merged pendant <pendant> reproduce byte for byte from its three
    inputs and its decisions file?

## Conditional writes

18. When a conditional write to <predicate> on <subject> was refused, what were
    the expected, current and proposed values?
