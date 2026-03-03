_ky_completion() {
	local cur prev words cword
	if declare -F _init_completion >/dev/null 2>&1; then
		_init_completion || return
	else
		cur="${COMP_WORDS[COMP_CWORD]}"
		if [[ $COMP_CWORD -gt 0 ]]; then
			prev="${COMP_WORDS[COMP_CWORD-1]}"
		else
			prev=""
		fi
		words=("${COMP_WORDS[@]}")
		cword=$COMP_CWORD
	fi

	local commands="status st start stop reload kill echo connect restart quit run init add serve ui self help version"
	local flags="--all -a --no-watch -W --daemon -d --stop --echo --restart --status -h --help -V --version"

	local config_path="${XDG_CONFIG_HOME:-$HOME/.config}/kagaya/projects"
	local projects=""
	if [[ -f "$config_path" ]]; then
		projects=$(grep -v '^#' "$config_path" 2>/dev/null | grep -v '^[[:space:]]*$' | cut -d: -f1 | tr -d ' ')
	fi

	if [[ $cword -eq 1 ]]; then
		COMPREPLY=( $(compgen -W "$commands $projects" -- "$cur") )
	else
		case "${words[1]}" in
			status|st|start|stop|reload|kill|echo|connect|restart|quit|run)
				COMPREPLY=( $(compgen -W "$projects $flags" -- "$cur") )
				;;
		add)
			if [[ $cur == -* ]]; then
				COMPREPLY=( $(compgen -W "--run" -- "$cur") )
			elif [[ $cword -eq 3 ]]; then
				COMPREPLY=( $(compgen -d -- "$cur") )
			fi
			;;
			*)
				if [[ $cur == -* ]]; then
					COMPREPLY=( $(compgen -W "$flags" -- "$cur") )
				else
					COMPREPLY=( $(compgen -W "$projects $commands" -- "$cur") )
				fi
				;;
		esac
	fi
}

complete -F _ky_completion ky
complete -F _ky_completion kagaya
