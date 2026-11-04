ofsht() {
    local _ofsht_command="" _ofsht_arg _ofsht_color_value=0
    for _ofsht_arg in "$@"; do
        if [[ "$_ofsht_color_value" == 1 ]]; then
            _ofsht_color_value=0
            continue
        fi
        case "$_ofsht_arg" in
            --color) _ofsht_color_value=1 ;;
            --color=*|--verbose|-v) ;;
            cd|add|rm|switch) _ofsht_command=$_ofsht_arg; break ;;
            *) command ofsht "$@"; return $? ;;
        esac
    done

    if [[ -z "$_ofsht_command" ]]; then
        command ofsht "$@"
        return $?
    fi

    for _ofsht_arg in "$@"; do
        case "$_ofsht_arg" in
            --) break ;;
            --help|--version) command ofsht "$@"; return $? ;;
            --*) ;;
            -*h*|-*V*) command ofsht "$@"; return $? ;;
        esac
    done

    local _ofsht_result
    _ofsht_result=$(command ofsht "$@") || return $?
    if [[ -n "$_ofsht_result" ]]; then
        cd -- "$_ofsht_result" || return $?
    fi
}
