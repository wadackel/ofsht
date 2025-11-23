# ofsht shell integration for Fish
#
# This function wraps the ofsht command to provide automatic directory
# changing for 'cd', 'add', and 'rm' subcommands.
#
# Usage:
#   Add this to your ~/.config/fish/config.fish:
#   ofsht shell-init fish | source

function ofsht
    # Handle cd, add, and rm subcommands with automatic directory changing
    if test "$argv[1]" = "cd"; or test "$argv[1]" = "add"; or test "$argv[1]" = "rm"
        set -l result (command ofsht $argv)
        or return $status
        if test -n "$result"
            cd -- $result
            or return $status
        end
    else
        # Pass through all other subcommands
        command ofsht $argv
    end
end
