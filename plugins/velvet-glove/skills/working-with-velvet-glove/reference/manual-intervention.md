# Manual intervention

When Velvet Glove blocks a Stop, the reason starts with
`velvet-glove found issues to fix before stopping:` and lists, per tool, the
affected files and an excerpt of the final check's output (ANSI-free, paths
relative to the project; `…truncated; full log: <path>` when cut). The files
it already fixed automatically are not listed as issues.

As the agent:

1. Fix the reported issues in the named files, then stop again; the next Stop
   rechecks them.
2. If an issue should not be fixed (the user asked for that code, or the rule
   does not fit), say so and stop; do not work around the hook. A Stop that
   follows a block with the same issues is not blocked again, and after three
   consecutive blocks Stop is always allowed. The files stay queued for the
   next turn.
3. Read the full log only when the excerpt is truncated and you need more.

Issues a tool reports only in files that were not changed this turn never
block; the user gets a one-line note instead. Tool crashes, missing tools,
timeouts, and policy errors never block by default; the user is told.

To change what counts as an issue, amend the tool in the project policy (see
[configuration.md](configuration.md)), for example to ignore a rule only in
the hook. `velvet-glove check FILES` reproduces the Stop-time verdict by hand.
