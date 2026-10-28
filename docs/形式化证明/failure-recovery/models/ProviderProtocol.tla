-------------------------- MODULE ProviderProtocol --------------------------
EXTENDS Naturals
CONSTANT Fixed
VARIABLES scenario, phase, corrupted, decision
vars == <<scenario, phase, corrupted, decision>>

Init == /\ scenario \in {"body_error", "event_auth", "null_success", "split_utf8", "malformed"}
        /\ phase = "received" /\ corrupted = FALSE /\ decision = "none"

Decode == /\ phase = "received" \* services/llm.rs:1134, byte stream yields data
          /\ phase' = "decoded"
          /\ corrupted' = (~Fixed /\ scenario = "split_utf8")
          /\ UNCHANGED <<scenario, decision>>

Validate == /\ phase = "decoded" \* services/llm.rs:1024,1812; services/llm/failure.rs:102; services/llm/stream_decode.rs:14
            /\ phase' = IF scenario = "body_error" THEN IF Fixed THEN "failure" ELSE "ready"
                        ELSE IF scenario = "event_auth" THEN "failure"
                        ELSE IF scenario = "null_success" THEN IF Fixed THEN "ready" ELSE "failure"
                        ELSE IF scenario = "malformed" THEN IF Fixed THEN "failure" ELSE "ready"
                        ELSE "ready"
            /\ UNCHANGED <<scenario, corrupted, decision>>

Classify == /\ phase = "failure" \* orchestrator/llm.rs:1322,1355; services/llm/failure.rs:12
            /\ phase' = "settled"
            /\ decision' = IF scenario = "event_auth" THEN IF Fixed THEN "permanent" ELSE "context"
                           ELSE IF scenario = "body_error" THEN "permanent"
                           ELSE IF scenario = "malformed" THEN "transient"
                           ELSE "other"
            /\ UNCHANGED <<scenario, corrupted>>

Accept == /\ phase = "ready" \* services/llm.rs:1041,1203,1919
          /\ phase' = "settled" /\ decision' = "success"
          /\ UNCHANGED <<scenario, corrupted>>

Next == Decode \/ Validate \/ Classify \/ Accept
Spec == Init /\ [][Next]_vars
TypeOK == /\ scenario \in {"body_error", "event_auth", "null_success", "split_utf8", "malformed"}
          /\ phase \in {"received", "decoded", "failure", "ready", "settled"}
          /\ corrupted \in BOOLEAN
          /\ decision \in {"none", "success", "permanent", "context", "transient", "other"}
\* A failed provider envelope cannot be returned as a successful answer (services/llm.rs:1024).
NoFailedPayloadSuccess == scenario = "body_error" => decision # "success"
\* A completed response with a null error field remains successful (services/llm/failure.rs:102).
NullSuccessRemainsSuccess == scenario = "null_success" /\ phase = "settled" => decision = "success"
\* Split network chunks do not change the successful text or tool arguments (stream_decode.rs:9).
Utf8ContentPreserved == scenario = "split_utf8" /\ decision = "success" => ~corrupted
\* Invalid UTF-8 or JSON is not concealed by a later DONE (services/llm.rs:1919; stream_decode.rs:20).
InvalidPayloadNeverSucceeds == scenario = "malformed" => decision # "success"
\* A permanent provider code takes priority over misleading message text (orchestrator/llm.rs:1335).
PermanentCodeNotReclassified == scenario = "event_auth" /\ phase = "settled" => decision = "permanent"
=============================================================================
