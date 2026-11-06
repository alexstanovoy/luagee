#!/usr/bin/env bash
# No test of this repository is #[ignore]d: a test nobody runs is a test nobody notices failing. Fails and names each
# file that holds one (a step of the gate that runs ignored tests would be the only exception, and there is none).
#
#   tools/ignored_audit.sh
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here"
bad=0
while IFS= read -r file; do
    case "$file" in
        *) echo "ignored_audit: $file has ignored tests that no gate step runs"; bad=1 ;;
    esac
done < <(git ls-files '*.rs' | xargs -r grep -l '#\[ignore' || true)
[ "$bad" = 0 ] && echo "ignored_audit: every ignored test has a step"
exit "$bad"
