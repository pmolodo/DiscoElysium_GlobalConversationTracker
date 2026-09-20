#!/usr/bin/env bash
# Reading this project's environment variables from bash, with the prefix supplied.
#
# Source it:
#
#     . "$(dirname "$0")/degct-env.sh"
#     degct_env SOME_NAME a-default
#     degct_env_is_set NOLIMIT && echo "limits off"
#
# WHY A HELPER RATHER THAN A CONVENTION. Every environment variable this project defines is
# prefixed DEGCT_ - see CLAUDE.md for the rule and docs/environment.md for the list - and a
# convention a person has to remember is one that grows exceptions. The whole reason the rule
# exists is that GROUPS is a bash BUILT-IN ARRAY: assigning to it looks like it works, `set -x`
# even shows the right value, and every later "$GROUPS" hands back a numeric gid. That cost
# time three separate times.
#
# So the prefix is applied here, by a function, and a new variable is named right because
# there is no other way to ask for one.
#
# AND IN BASH THE RULE COVERS LOCALS TOO, not just exported variables, because the collision
# that started this was a local. A script whose every variable is DEGCT_ prefixed cannot
# shadow anything the shell owns.

# One of ours, by its BARE name, with a default: `degct_env ROW_SECONDS 600`.
degct_env() {
    local degct_name="DEGCT_$1"
    local degct_default="${2-}"
    printf '%s' "${!degct_name:-$degct_default}"
}

# Whether one of ours is set at all, whatever it is set to.
#
# The shape a flag takes here: several measurements switch on PRESENCE rather than value, so
# DEGCT_NOLIMIT=1 and DEGCT_NOLIMIT= mean the same thing and neither has to be parsed.
degct_env_is_set() {
    local degct_name="DEGCT_$1"
    [ -n "${!degct_name+set}" ]
}

# Export one of ours for a child process: `degct_env_set CONVERSATION 631`.
degct_env_set() {
    export "DEGCT_$1=$2"
}

# A variable somebody else owns, read under its own name - PATH, CARGO_TARGET_DIR,
# NUMBER_OF_PROCESSORS. Spelled differently from `degct_env` so which of the two a call means
# is visible where it is called.
degct_env_foreign() {
    local degct_name="$1"
    local degct_default="${2-}"
    printf '%s' "${!degct_name:-$degct_default}"
}
