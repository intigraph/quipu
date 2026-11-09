# Profiled merge sequence: focused transcript

This is a retained readable transcript, not a video or a matched-base benchmark.
The target is the source library at `f315c333c4bbbd41e06a0b7d4e68379a09e09cab`,
running synthetic in-memory fixtures. It is not an installed service.

The trigger is either a convergent explicit-value decision with no receive
operations, or a partial merge that holds every slot. The source before the
fix called the receive transaction unconditionally; that base was not executed
in this recording. The two observed controls require absent receive IDs,
actual decide/record transaction counts, and unchanged ROOT for the held case.
Both passed under the HOME guard. This does not qualify every profiled mode.

[Actual focused output](focused-transcript.txt) includes repeat commands with
the executable path omitted. Compiler warnings and private paths are omitted;
test result lines are retained from the admitted run. No raw private log is
published. There is no replayable recording: the serialized Cargo admission
covered the retained run, not an additional recording run.

The publication refresh adds current main and formatting/source witnesses.
The pending writer and these test bodies have no whitespace-independent delta
from the demonstrated revision. Hosted CI must qualify the refreshed head.
Other retained fixtures cover list lowering and schema behavior separately;
this transcript does not claim their results or production adoption.
