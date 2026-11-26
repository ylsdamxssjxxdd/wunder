#!/usr/bin/env bash
# Run the instrumented smoke tests against a booted emulator.
#
# reactivecircus/android-emulator-runner executes every line of its `script`
# input as a separate process, so a `cd` on one line never reaches the gradle
# command on the next (and `status=$?` / shell redirection do not work either).
# Keep the whole flow in one script and invoke it as a single line.
#
# FAILURE REPORTING: a bare "test failed" cannot tell a launch crash from a
# failed assertion, and the on-device log is unreadable afterwards, so always
# copy logcat and the app's own crash reports into $GITHUB_WORKSPACE.
set -uo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd -P)"
ANDROID_DIR="$ROOT/crates/wunder-android/android"
WORKSPACE="${GITHUB_WORKSPACE:-$ROOT}"
GRADLE="${GRADLE:-gradle}"

cd "$ANDROID_DIR" || exit 2

"$GRADLE" -PwunderAbis=x86_64 connectedDebugAndroidTest
status=$?

adb logcat -d -v threadtime > "$WORKSPACE/android-logcat.txt" 2>&1 || true
adb exec-out run-as dev.wunder.android tar -cf - files/wunder/logs \
    > "$WORKSPACE/android-crash-logs.tar" 2>/dev/null || true

exit "$status"
