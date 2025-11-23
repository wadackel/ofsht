# ofsht shell integration for Zsh
#
# This function wraps the ofsht command to provide automatic directory
# changing for 'cd', 'add', and 'rm' subcommands.
#
# Usage:
#   Add this to your ~/.zshrc:
#   eval "$(ofsht shell-init zsh)"

ofsht() {
    # Handle cd, add, and rm subcommands with automatic directory changing
    if [[ "$1" == "cd" ]] || [[ "$1" == "add" ]] || [[ "$1" == "rm" ]]; then
        local result
        result=$(command ofsht "$@") || return $?
        [[ -n "$result" ]] && cd -- "$result"
    else
        # Pass through all other subcommands
        command ofsht "$@"
    fi
}
