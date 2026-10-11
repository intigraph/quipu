# Direct workflow processing

Direct workflows use the existing `Workflow`, `WorkflowDefinition`,
`WorkflowStep`, `WorkflowRun` and `TransitionEvent` entities. A definition is
one entity typed as both `Workflow` and `WorkflowDefinition`; a declared step
is typed as both `Step` and `WorkflowStep`. Labels retain the existing shapes.
No local journal is the authority for this processing path.

`enforce_workflows` in `[quipu.governance]` defaults to false. Enabling it is a
separate deployment decision: legacy definitions and active runs must be
migrated before enrollment. A client must check `GET /workflow/capabilities`
and refuse writes unless processing is enabled and signature version 2 is
supported. The read exposes processing capability, not keys or configuration.
Source availability alone does not establish an enabled guard.

In the same graph, a definition declares `initialState`, `terminalState`, and
`hasStep`. Each step declares one `fromState` and `toState`. The combination of
step name and starting state must be unambiguous. A terminal state cannot have
an outgoing step. Definitions and steps cannot change their committed process;
use a new identity for a revised plan. Existing shapes keep optional fields for
legacy records, while enrolled processing requires them.

A run starts at the definition's initial state with `currentRevision` zero.
An advance must atomically write a new signed event and replace both
`currentState` and `currentRevision`. Its `fromRevision` must equal the current
revision, and the resulting run revision must be exactly one higher. The event
must name a step declared by that run's definition and match its states.
Committed events are immutable. Steps, definitions and runs from another graph
cannot substitute for those in the authoritative graph. Identity registrations
retain their existing authority and key-rotation rules.

For enrolled processing, signatures bind the expected revision:

```text
shuttle-transition-v2|{run}|{step}|{from}|{to}|{at}|{agent}|{from_revision}
```

`transitionVersion` is integer 2. Revision values use canonical nonnegative
decimal encoding. The revision prevents an old advance being reused when a
loop returns to the same state. Legacy v1 signatures remain supported outside
enrolled processing; the v1 message format does not change. Both client and
server must adopt v2 before enrollment.

Use SPARQL Update on `/update`: its WHERE clause compares the current state
and revision; its DELETE/INSERT replaces them together with the signed event.
The same native transaction guard covers other write surfaces. A refused
operation leaves the run and event facts unchanged. A retry whose WHERE clause
no longer matches is a no-op. After an indeterminate response, read the exact
event identity and run revision, validate the immutable payload, and do not
mint a new event merely because the response was lost.

The native controls include signatures, illegal steps, missing atomic state
changes, exact replay, graph isolation, plan/event immutability, initial state,
and a stale advance after a state cycle. Server controls exercise both normal
and full-copy SPARQL Update paths, concurrent compare-and-set, lost-response
replay, and the capability response. These are source tests, not installed
workflow, priority-consumer or scheduled notification acceptance.

Before activation, adopt reviewed source and the shared property catalogue,
verify the installed guard, migrate active workflows and clients, and retain a
named rollback. `fromState` and `toState` are shared by declared steps and
observed events, so their catalogue must not materialize a Step as a
TransitionEvent. Do not enable processing while legacy writers still mutate
a run or batch-export v1 transitions as its primary state.
