# ofsht shell integration for Bash
#
# This function wraps the ofsht command to provide automatic directory
# changing for 'cd', 'add', and 'rm' subcommands.
#
# Usage:
#   Add this to your ~/.bashrc:
#   eval "$(ofsht shell-init bash)"

ofsht() {
    # Handle cd, add, and rm subcommands with automatic directory changing
    if [[ "$1" == "cd" ]] || [[ "$1" == "add" ]] || [[ "$1" == "rm" ]]; then
        local result
        result=$(command ofsht "$@") || return $?
        if [[ -n "$result" ]]; then
            cd -- "$result" || return $?
        fi
    else
        # Pass through all other subcommands
        command ofsht "$@"
    fi
}
