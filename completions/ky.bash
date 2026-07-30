_ky_services() {
	local config="${XDG_CONFIG_HOME:-$HOME/.config}/kagaya/projects.toml"
	[[ -f $config ]] || return
	awk -F'[]=[]' '/^[a-zA-Z0-9_-]+[ \t]*=/ {gsub(/[ \t]/,"",$1); print $1} /^\[[a-zA-Z0-9_-]+\]/ {print $2}' "$config"
}

_ky() {
	local cur commands
	COMPREPLY=()
	cur="${COMP_WORDS[COMP_CWORD]}"
	commands="status st start stop restart logs echo show add remove rm init autostart reload-config rc serve self all help version"

	if [[ $COMP_CWORD -eq 1 ]]; then
		COMPREPLY=($(compgen -W "$commands $(_ky_services)" -- "$cur"))
		return
	fi

	case "${COMP_WORDS[1]}" in
		start)
			COMPREPLY=($(compgen -W "$(_ky_services) --all --wait --force --autostart --echo --detailed --no-watch" -- "$cur"))
			;;
		stop | restart | status | st | logs | echo | show | remove | rm)
			COMPREPLY=($(compgen -W "$(_ky_services) --all" -- "$cur"))
			;;
		serve)
			COMPREPLY=($(compgen -W "stop restart status daemon foreground" -- "$cur"))
			;;
		autostart)
			COMPREPLY=($(compgen -W "$(_ky_services) on off" -- "$cur"))
			;;
		self)
			COMPREPLY=($(compgen -W "update" -- "$cur"))
			;;
	esac
}

complete -F _ky ky
complete -F _ky kagaya
