function ofsht
    set -l _ofsht_command ""
    set -l _ofsht_color_value 0
    for _ofsht_arg in $argv
        if test $_ofsht_color_value = 1
            set _ofsht_color_value 0
            continue
        end
        switch "$_ofsht_arg"
            case --color
                set _ofsht_color_value 1
            case '--color=*' --verbose -v
            case cd add rm switch
                set _ofsht_command $_ofsht_arg
                break
            case '*'
                command ofsht $argv
                return $status
        end
    end

    if test -z "$_ofsht_command"
        command ofsht $argv
        return $status
    end

    for _ofsht_arg in $argv
        switch "$_ofsht_arg"
            case --
                break
            case --help --version
                command ofsht $argv
                return $status
            case '--*'
            case '-*h*' '-*V*'
                command ofsht $argv
                return $status
        end
    end

    set -l _ofsht_result (command ofsht $argv)
    or return $status
    if test -n "$_ofsht_result"
        cd -- "$_ofsht_result"
        or return $status
    end
end
