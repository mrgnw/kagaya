#compdef ky kagaya

_ky_services() {
	local config="${XDG_CONFIG_HOME:-$HOME/.config}/kagaya/projects.toml"
	[[ -f $config ]] || return
	awk -F'[]=[]' '/^[a-zA-Z0-9_-]+[ \t]*=/ {gsub(/[ \t]/,"",$1); print $1} /^\[[a-zA-Z0-9_-]+\]/ {print $2}' "$config"
}

_ky() {
	local -a commands services
	commands=(
		'status:show service status'
		'st:show service status (alias)'
		'start:start service(s)'
		'stop:stop service(s)'
		'restart:restart service(s) or a process'
		'logs:show log file paths'
		'echo:tail + stream live output'
		'show:show service config'
		'add:register a service'
		'remove:unregister a service'
		'rm:unregister a service (alias)'
		'init:create config files'
		'autostart:start service(s) on login'
		'reload-config:re-sync plists from config'
		'rc:re-sync plists (alias)'
		'serve:web UI daemon'
		'self:self-management (update)'
		'all:status for all services'
		'help:show help'
		'version:show version'
	)

	if (( CURRENT == 2 )); then
		services=(${(f)"$(_ky_services)"})
		_describe -t commands 'command' commands
		(( ${#services} )) && _describe -t services 'service' services
		return
	fi

	case $words[2] in
		start)
			_arguments '--all[start all]' '--wait[block until ready]' '--force[kill port holders]' \
				'--autostart[only autostart services]' '--echo[stream output after]' \
				'--detailed[per-process detail]' '--no-watch[skip status watch]'
			services=(${(f)"$(_ky_services)"})
			(( ${#services} )) && _describe -t services 'service' services
			;;
		stop|restart|status|st|logs|echo|show|remove|rm)
			services=(${(f)"$(_ky_services)"})
			(( ${#services} )) && _describe -t services 'service' services
			;;
		serve)
			_values 'action' stop restart status daemon foreground
			;;
		autostart)
			services=(${(f)"$(_ky_services)"} on off)
			_describe -t services 'service or on/off' services
			;;
		self)
			_values 'action' update
			;;
	esac
}

_ky "$@"
