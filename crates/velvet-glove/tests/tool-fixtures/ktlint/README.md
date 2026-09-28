No `operational-failure` case: verified at 1.8.0, ktlint exits 1 for lint
issues, an unresolvable path, an unknown flag, and a malformed
`.editorconfig` alike. There is no exit code that distinguishes "needs a
real fix" from "the tool setup is broken," so no case can assert an
`operational` outcome without being misleading. See the doc comment in
`tools/ktlint.pkl` for the verified transcript.
