# Working in this repository

## Running commands: tee anything that might be slow, and say where the log is

**Anything that might even potentially be slow goes through `tools/run-logged.sh`, and the
reply says the log path.** Builds, test runs, measurements, the extractor, deploys, any
script whose runtime is not obviously instant. When in doubt, log it: the cost is one
wrapper, and the run that turns out to be worth reading is rarely the one that looked like
it would be.

Genuinely instant things - `ls`, `git status`, reading a file - do not need it.

```sh
tools/run-logged.sh cargo full-suite -- cargo test --release
DEGCT_RUN_LOG_DIR=measurements/logs \
  DEGCT_CONVERSATION=631 tools/run-logged.sh cargo shared-symbolic -- \
  cargo run --release --example shared_symbolic
```

It tees the whole output to `testing/logs/<date>_<time>_<revision>_<tool>_<verb>.txt`
(`RUN_LOG_DIR` moves that; the measurement scripts point it at `measurements/logs`) and
passes the command's own exit status straight through, so it drops in wherever the bare
command stood. `--name-only` prints the path without running anything.

**Never pipe such a command into `grep`, `tail`, `head`, `sort` or anything else as the
only thing that receives its output.** A filter keeps the few lines it matched and throws
the rest away, so a run that took forty minutes leaves nothing to look at when the
interesting part turns out to be somewhere the filter did not look - a panic, a progress
line, the row before the one that failed. Filter the LOG afterwards instead; it is still
there.

Say the log path in the reply, every time, so a run can be followed while it is still
going rather than waited out. A run whose log path was never mentioned is one nobody else
can watch.

**Give the path WHOLE and copy-pastable.** These names carry a full commit hash, so the
temptation is to elide the middle - `2026-09-05_03,51,18_..._cargo_full-suite.txt`. Do not:
an elided path cannot be pasted into anything, which defeats the point of saying it. Print
it verbatim, on its own line, in a code block rather than inline prose.

This file and `CLAUDE.md` carry the same rules and are kept in step. They are duplicated
rather than one pointing at the other because the Git rule below overrides a built-in
default, and an override that goes missing because a pointer was not followed is worse
than two copies to keep aligned.

## Environment variables: name every one of ours `DEGCT_`

**Every environment variable this project defines is prefixed `DEGCT_`.** All languages, all
scripts - Rust, Python, bash, PowerShell.

**A throwaway script in a session scratchpad uses `DEGCTT_` instead** - the same rule with a
second `T` for temporary. Temp scripts are where the convention is most tempting to skip and
where the bug is hardest to see: nobody reviews them, nothing tests them, and they are deleted
before anyone asks what went wrong. The separate prefix also keeps them visibly apart from the
committed drivers, so a stray value exported by a scratch script can never be mistaken for a
setting one of the real ones reads.

Variables we merely READ from the world keep their own names: `PATH`, `CARGO_TARGET_DIR`,
`NUMBER_OF_PROCESSORS`. The rule is about names we invent.

**Why, and it is not hypothetical - it has cost time three times.** `GROUPS` is a bash
built-in array holding the user's numeric group ids. Assigning to it looks like it works, and
`set -x` will even show the assignment with the right value; every later `"$GROUPS"` then
expands to `${GROUPS[0]}`, a gid. In this repository that is a number the measurement takes
for a conversation id, so a run refuses with "conversation 197609: no group builds from it"
and nothing points at the variable.

The shell owns a long list of short generic names - `GROUPS`, `IFS`, `HOME`, `LINES`,
`COLUMNS`, `SECONDS`, `RANDOM`, `PWD`, `REPLY`, `PIPESTATUS`, `UID`, `HOSTNAME` - and they are
drawn from exactly the vocabulary a measurement wants. A prefix takes the whole class of
collision off the table rather than dodging them one at a time.

See de-12wr.9 for the rename of the names that predate this rule.

## Git

### Top-level agent

**Commit to the branch that is checked out**, including when that branch is `main`. Do
not create a branch first.

This is written down because Claude Code's built-in default is the opposite - "if on the
default branch, branch first" - and it does not suit this repository, whose history is a
single line of commits on `main`. A branch per change is noise here rather than
isolation.

### Sub-agents

Always work on your own branch in your own worktree.
