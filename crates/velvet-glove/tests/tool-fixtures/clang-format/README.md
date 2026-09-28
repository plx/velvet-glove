No `operational-failure` case: verified at 23.1.2, clang-format exits 1 for
a real formatting issue AND for every operational failure this spec can
trigger without a wrapper program (missing file, unrecognized flag, an
invalid `.clang-format` style config all exit 1). There is no exit code that
distinguishes "needs a real fix" from "the tool setup is broken," so no case
can assert an `operational` outcome without being misleading. See the doc
comment in `tools/clang_format.pkl` for the verified transcript.
