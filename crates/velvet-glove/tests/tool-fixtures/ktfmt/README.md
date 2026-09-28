No `operational-failure` case: verified at 0.59 (npm distribution, matching
upstream Meta ktfmt), `--dry-run --set-exit-if-changed` exits 1 for a real
formatting issue, a syntax error ktfmt can't parse, and an unrecognized
flag alike. There is no exit code that distinguishes "needs a real fix"
from "the tool setup is broken," so no case can assert an `operational`
outcome without being misleading. See the doc comment in `tools/ktfmt.pkl`
for the verified transcript.
