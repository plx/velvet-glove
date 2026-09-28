No `operational-failure` case: verified at 2.0.2, cpplint exits 1 for both a
real style violation and a usage error (unrecognized flag). A broken
`CPPLINT.cfg` (unknown config key) only warns and still exits 0 after
linting normally, and a missing input file silently exits 0 too. There is no
exit code that distinguishes "needs a real fix" from "the tool setup is
broken," so no case can assert an `operational` outcome without being
misleading. See the doc comment in `tools/cpp_lint.pkl` for the verified
transcript.
