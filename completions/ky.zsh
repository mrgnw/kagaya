#compdef ky kagaya

_ky() {
	local -a commands projects flags
	local config_path="${XDG_CONFIG_HOME:-$HOME/.config}/kagaya/projects"

	commands=(
		'status:show project status'
		'st:show project status (alias)'
		'start:start project(s)'
		'stop:stop project(s)'
		'reload:restart project(s)'
		'kill:kill process(es)'
		'echo:view logs'
		'connect:connect to process'
		'restart:restart process(es)'
		'quit:quit daemon'
		'run:run command'
		'init:create config file'
		'add:add a project'
		'serve:start web UI'
		'ui:start web UI (alias)'
		'self:self-management commands'
		'help:show help'
		'version:show version'
	)

	flags=(
		'--all:target all projects'
		'-a:target all projects'
		'--daemon:run in background'
		'-d:run in background'
		'--stop:stop daemon'
		'--echo:view daemon logs'
		'--restart:restart daemon'
		'--status:show daemon status'
		'-h:show help'
		'--help:show help'
		'-V:show version'
		'--version:show version'
	)

	if [[ -f "$config_path" ]]; then
		projects=("${(@f)$(grep -v '^#' "$config_path" 2>/dev/null | grep -v '^[[:space:]]*$' | cut -d: -f1 | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')}")
	fi

	local state
	_arguments -C \
		'1: :->command' \
		'*:: :->args' && return 0

	case $state in
		command)
			_describe -t commands 'command' commands
			_describe -t projects 'project' projects
			;;
		args)
			case $words[1] in
				status|st|start|stop|reload|kill|echo|connect|restart|quit|run)
					_describe -t projects 'project' projects
					_describe -t flags 'flag' flags
					;;
			add)
				if [[ ${words[(i)--run]} -le $CURRENT ]]; then
					return
				fi
				_arguments '*--run[register a standalone command]:command:' && return
				if [[ $CURRENT -eq 3 ]]; then
					_path_files -/
				fi
				;;
			serve|ui)
				_describe -t flags 'flag' flags
				;;
			self)
				local -a self_commands
				self_commands=('update:update kagaya to the latest version')
				_describe -t self_commands 'self command' self_commands
				;;
				*)
					_describe -t projects 'project' projects
					_describe -t commands 'command' commands
					;;
			esac
			;;
	esac
}

_ky "$@"
