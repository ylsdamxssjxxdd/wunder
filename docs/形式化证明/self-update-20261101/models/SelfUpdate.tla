--------------------------- MODULE SelfUpdate ---------------------------
(*** Bounded model of the desktop app's online self-update pipeline.      ***)
(*** One staged slot serves three targets (desktop exe / cli exe /        ***)
(*** supplement archive) from two sources; the model checks the phase     ***)
(*** machine guards, the download verification gate, the desktop rename   ***)
(*** rotation with its crash windows, staged-package restore at process   ***)
(*** start and per-target dispatch.  Fixed = FALSE models a plausible     ***)
(*** shortcut that skips the download hash check; Fixed = TRUE models     ***)
(*** the shipped code.  The cli rotation is abstracted into one step:     ***)
(*** it shares the desktop rotation's copy-verify + rename shape.         ***)
EXTENDS Naturals

CONSTANTS Fixed, CurrentVersion, RemoteVersion
VARIABLES phase, target, staged, staged_target, staged_version,
          staged_verified, applying, exe_state, exe_version,
          exe_old, exe_new

vars == <<phase, target, staged, staged_target, staged_version,
          staged_verified, applying, exe_state, exe_version,
          exe_old, exe_new>>

Targets == {"desktop", "cli", "supplement"}

\* The checked target's locally detected version: the desktop knows its own
\* compiled version; cli and supplement read as unknown (always offerable).
LocalVersion == IF target = "desktop" THEN exe_version ELSE 0
\* crates/wunder-desktop/native_update.rs:353-362

\* A staged package of the checked target that the offering does not beat
\* keeps the card in the staged phase instead of re-offering the download.
StagedKeeps ==
    /\ staged = "pkg"                     \* crates/wunder-desktop/native_update.rs:205-209
    /\ staged_target = target             \* crates/wunder-desktop/native_update.rs:209
    /\ RemoteVersion <= staged_version    \* crates/wunder-desktop/native_update.rs:210-213

\* A pending package may come back after a process start: cli and
\* supplement packages have no compile-time baseline, a desktop package
\* only while strictly newer than the running exe.
StagedRestorable ==
    /\ staged = "pkg"                     \* crates/wunder-desktop/native_update.rs:600-603
    /\ \/ staged_target /= "desktop"      \* crates/wunder-desktop/native_update.rs:746-750
       \/ staged_version > exe_version    \* crates/wunder-desktop/native_update.rs:746-750

Init ==
    /\ phase = "idle"
    /\ target = "desktop"
    /\ staged = "none"
    /\ staged_target = "desktop"
    /\ staged_version = 0
    /\ staged_verified = FALSE
    /\ applying = "none"
    /\ exe_state = "present"
    /\ exe_version = CurrentVersion
    /\ exe_old = FALSE
    /\ exe_new = "absent"

\* The reader clicks 检查更新; the card refuses a second operation while
\* one is in flight, and the auto-check settles on a target in turn (the card
\* no longer carries a hand-picked target).  Check still picks one target
\* nondeterministically, matching the per-target loop in update_check_auto.
Check ==
    /\ phase \in {"idle", "up_to_date", "available", "staged", "applied", "failed"}
                                          \* crates/wunder-desktop/native_update.rs:346-347
    /\ target' \in Targets                \* crates/wunder-desktop/native_update.rs:341
    /\ phase' = "checking"                \* crates/wunder-desktop/native_update.rs:349
    /\ UNCHANGED <<staged, staged_target, staged_version, staged_verified,
                  applying, exe_state, exe_version, exe_old, exe_new>>

\* The source answers with a newer release and no staged package keeps the
\* card: the offering becomes available for download.
OfferNewer ==
    /\ phase = "checking"                 \* crates/wunder-desktop/native_update.rs:349
    /\ RemoteVersion > LocalVersion       \* crates/wunder-desktop/native_update.rs:214-219
    /\ ~StagedKeeps                       \* crates/wunder-desktop/native_update.rs:205-213
    /\ phase' = "available"               \* crates/wunder-desktop/native_update.rs:222-231
    /\ UNCHANGED <<target, staged, staged_target, staged_version,
                  staged_verified, applying, exe_state, exe_version,
                  exe_old, exe_new>>

\* The source answers with a release the target already has.
OfferNone ==
    /\ phase = "checking"                 \* crates/wunder-desktop/native_update.rs:349
    /\ ~StagedKeeps                       \* crates/wunder-desktop/native_update.rs:205-213
    /\ phase' = "up_to_date"              \* crates/wunder-desktop/native_update.rs:232-234
    /\ UNCHANGED <<target, staged, staged_target, staged_version,
                  staged_verified, applying, exe_state, exe_version,
                  exe_old, exe_new>>

\* The offering is not newer than what is already staged for this target:
\* the staged package wins and the card stays pending.
OfferStaged ==
    /\ phase = "checking"                 \* crates/wunder-desktop/native_update.rs:349
    /\ StagedKeeps                        \* crates/wunder-desktop/native_update.rs:205-213
    /\ phase' = "staged"                  \* crates/wunder-desktop/native_update.rs:220-221
    /\ UNCHANGED <<target, staged, staged_target, staged_version,
                  staged_verified, applying, exe_state, exe_version,
                  exe_old, exe_new>>

\* The reader clicks 下载更新; only the available phase allows it.
Download ==
    /\ phase = "available"                \* crates/wunder-desktop/native_update.rs:409-410
    /\ phase' = "downloading"             \* crates/wunder-desktop/native_update.rs:415
    /\ UNCHANGED <<target, staged, staged_target, staged_version,
                  staged_verified, applying, exe_state, exe_version,
                  exe_old, exe_new>>

\* The stream is hashed while writing and the size checked; success lands
\* the package in staging with its verification verdict.  Fixed = FALSE is
\* the shortcut that records a package without the hash verdict.
DownloadVerify ==
    /\ phase = "downloading"              \* crates/wunder-desktop/native_update.rs:415
    /\ staged' = "pkg"                    \* crates/wunder-desktop/native_update.rs:501-506
    /\ staged_target' = target            \* crates/wunder-desktop/native_update.rs:503
    /\ staged_version' = RemoteVersion    \* crates/wunder-desktop/native_update.rs:504
    /\ staged_verified' = Fixed           \* crates/wunder-desktop/native_update.rs:455-465
    /\ phase' = "staged"                  \* crates/wunder-desktop/native_update.rs:506
    /\ UNCHANGED <<target, applying, exe_state, exe_version,
                  exe_old, exe_new>>

\* A failed download only removes the shared part file; a package already
\* staged for another target stays intact and applicable.
DownloadFail ==
    /\ phase = "downloading"              \* crates/wunder-desktop/native_update.rs:466-470
    /\ phase' = "failed"                  \* crates/wunder-desktop/native_update.rs:473-478
    /\ UNCHANGED <<target, staged, staged_target, staged_version,
                  staged_verified, applying, exe_state, exe_version,
                  exe_old, exe_new>>

\* The reader clicks 应用/导入; dispatch always follows the staged
\* package's own target, never the card's current selection.
ApplyDispatch ==
    /\ phase \in {"staged", "failed"}     \* crates/wunder-desktop/native_update.rs:517-526
    /\ staged = "pkg"                     \* crates/wunder-desktop/native_update.rs:517-525
    /\ applying' = staged_target          \* crates/wunder-desktop/native_update.rs:564-581
    /\ phase' = "applying"                \* crates/wunder-desktop/native_update.rs:527
    /\ UNCHANGED <<target, staged, staged_target, staged_version,
                  staged_verified, exe_state, exe_version, exe_old, exe_new>>

\* Desktop rotation step 1: the staged package is copied beside the exe and
\* re-hashed there, so rotation never proceeds on unverified bytes.
RotateStage ==
    /\ phase = "applying"                 \* crates/wunder-desktop/native_update.rs:634
    /\ applying = "desktop"               \* crates/wunder-desktop/native_update.rs:576-579
    /\ exe_new = "absent"                 \* crates/wunder-desktop/native_update.rs:645-648
    /\ staged_verified                    \* crates/wunder-desktop/native_update.rs:645
    /\ exe_new' = "ready"                 \* crates/wunder-desktop/native_update.rs:645
    /\ UNCHANGED <<phase, target, staged, staged_target, staged_version,
                  staged_verified, applying, exe_state, exe_version, exe_old>>

\* Desktop rotation step 2: the running exe is renamed to its backup; this
\* is the only window in which the program itself is missing.
RotateBackup ==
    /\ phase = "applying"                 \* crates/wunder-desktop/native_update.rs:634
    /\ applying = "desktop"               \* crates/wunder-desktop/native_update.rs:576-579
    /\ exe_new = "ready"                  \* crates/wunder-desktop/native_update.rs:645
    /\ exe_state = "present"              \* crates/wunder-desktop/native_update.rs:649
    /\ exe_old' = TRUE                    \* crates/wunder-desktop/native_update.rs:647-649
    /\ exe_state' = "absent"              \* crates/wunder-desktop/native_update.rs:649
    /\ UNCHANGED <<phase, target, staged, staged_target, staged_version,
                  staged_verified, applying, exe_version, exe_new>>

\* Desktop rotation step 3: the verified copy takes the exe's place.
RotateActivate ==
    /\ phase = "applying"                 \* crates/wunder-desktop/native_update.rs:634
    /\ applying = "desktop"               \* crates/wunder-desktop/native_update.rs:576-579
    /\ exe_state = "absent"               \* crates/wunder-desktop/native_update.rs:649
    /\ exe_new = "ready"                  \* crates/wunder-desktop/native_update.rs:645
    /\ exe_state' = "present"             \* crates/wunder-desktop/native_update.rs:653-656
    /\ exe_version' = staged_version      \* crates/wunder-desktop/native_update.rs:653-656
    /\ exe_new' = "absent"                \* crates/wunder-desktop/native_update.rs:653-656
    /\ UNCHANGED <<phase, target, staged, staged_target, staged_version,
                  staged_verified, applying, exe_old>>

\* The cli binary rotates through the same verified copy + rename shape and
\* takes effect on its next launch; a running TUI keeps the old file.
ConsumeInPlace ==
    /\ phase = "applying"                 \* crates/wunder-desktop/native_update.rs:674
    /\ applying = "cli"                   \* crates/wunder-desktop/native_update.rs:571-574
    /\ staged_verified                    \* crates/wunder-desktop/native_update.rs:678
    /\ staged' = "none"                   \* crates/wunder-desktop/native_update.rs:550-556
    /\ phase' = "applied"                 \* crates/wunder-desktop/native_update.rs:558
    /\ UNCHANGED <<target, staged_target, staged_version, staged_verified,
                  applying, exe_state, exe_version, exe_old, exe_new>>

\* The supplement archive feeds the existing import pipeline (validated
\* streaming extraction, overwrite merge, hot tool-env refresh); the exe is
\* never touched and the update is effective immediately.
ApplyImport ==
    /\ phase = "applying"                 \* crates/wunder-desktop/native_update.rs:702
    /\ applying = "supplement"            \* crates/wunder-desktop/native_update.rs:575
    /\ staged' = "none"                   \* crates/wunder-desktop/native_update.rs:550-556
    /\ phase' = "applied"                 \* crates/wunder-desktop/native_update.rs:558
    /\ UNCHANGED <<target, staged_target, staged_version, staged_verified,
                  applying, exe_state, exe_version, exe_old, exe_new>>

\* Process boundary with a pending package worth keeping: startup hygiene
\* clears rotation leftovers and restores the staged package.
RestartRestore ==
    /\ exe_state = "present"              \* crates/wunder-desktop/native_update.rs:585
    /\ StagedRestorable                   \* crates/wunder-desktop/native_update.rs:603
    /\ exe_old' = FALSE                   \* crates/wunder-desktop/native_update.rs:588-595
    /\ exe_new' = "absent"                \* crates/wunder-desktop/native_update.rs:588-595
    /\ phase' = "staged"                  \* crates/wunder-desktop/native_update.rs:616
    /\ applying' = "none"                 \* crates/wunder-desktop/native_update.rs:585
    /\ UNCHANGED <<target, staged, staged_target, staged_version,
                  staged_verified, exe_state, exe_version>>

\* Process boundary without a restorable package: staging is cleared, which
\* is also how a relaunch after a desktop rotation consumes the applied
\* package (its version is no longer newer than the running exe).
RestartClear ==
    /\ exe_state = "present"              \* crates/wunder-desktop/native_update.rs:585
    /\ ~StagedRestorable                  \* crates/wunder-desktop/native_update.rs:603
    /\ exe_old' = FALSE                   \* crates/wunder-desktop/native_update.rs:588-595
    /\ exe_new' = "absent"                \* crates/wunder-desktop/native_update.rs:588-595
    /\ phase' = "idle"                    \* crates/wunder-desktop/native_update.rs:618-627
    /\ staged' = "none"                   \* crates/wunder-desktop/native_update.rs:618-627
    /\ applying' = "none"                 \* crates/wunder-desktop/native_update.rs:585
    /\ UNCHANGED <<target, staged_target, staged_version, staged_verified,
                  exe_state, exe_version>>

\* The process dies in the one window where the exe is renamed away: the
\* machine cannot boot by itself and only the .old backup restores it.
KillAfterBackup ==
    /\ phase = "applying"                 \* crates/wunder-desktop/native_update.rs:634
    /\ applying = "desktop"               \* crates/wunder-desktop/native_update.rs:576-579
    /\ exe_state = "absent"               \* crates/wunder-desktop/native_update.rs:649
    /\ UNCHANGED vars

Next ==
    \/ Check \/ OfferNewer \/ OfferNone \/ OfferStaged
    \/ Download \/ DownloadVerify \/ DownloadFail
    \/ ApplyDispatch \/ RotateStage \/ RotateBackup \/ RotateActivate
    \/ ConsumeInPlace \/ ApplyImport
    \/ RestartRestore \/ RestartClear \/ KillAfterBackup

Spec == Init /\ [][Next]_vars

\* 暂存区里的待应用包一定是下载时通过清单哈希校验的包。
StagedAlwaysVerified == staged = "pkg" => staged_verified
\* crates/wunder-desktop/native_update.rs:455-465, 501-506

\* 程序本体缺失的任何时刻，.old 备份必然存在，可手工恢复。
ExeRestorable == exe_state = "absent" => exe_old
\* crates/wunder-desktop/native_update.rs:647-656

\* 轮换只有在包通过校验后才会开始：复制落盘时再次哈希校验。
RotationRequiresVerified == exe_new = "ready" => staged_verified
\* crates/wunder-desktop/native_update.rs:645

\* 只有蛹本体的更新会短暂摘下程序本体；舵机与补充包的应用不动 exe。
OnlyDesktopRotationTouchesExe == exe_state = "absent" => applying = "desktop"
\* crates/wunder-desktop/native_update.rs:564-581, 674, 702

\* 启动恢复从不会把蛹本体正在运行的同一版本再次端上待应用。
RestoreNoSameVersion ==
    (phase = "staged" /\ staged_target = "desktop") => staged_version > exe_version
\* crates/wunder-desktop/native_update.rs:746-750
=============================================================================
