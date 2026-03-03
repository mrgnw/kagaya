function __ky_projects
	set -l config_path "$HOME/.config/kagaya/projects"
	if set -q XDG_CONFIG_HOME
		set config_path "$XDG_CONFIG_HOME/kagaya/projects"
	end

	if test -f $config_path
		grep -v '^#' $config_path 2>/dev/null | grep -v '^[[:space:]]*$' | cut -d: -f1 | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
	end
end

complete -c ky -f
complete -c kagaya -f

complete -c ky -n "__fish_use_subcommand" -a "status" -d "show project status"
complete -c ky -n "__fish_use_subcommand" -a "st" -d "show project status (alias)"
complete -c ky -n "__fish_use_subcommand" -a "start" -d "start project(s)"
complete -c ky -n "__fish_use_subcommand" -a "stop" -d "stop project(s)"
complete -c ky -n "__fish_use_subcommand" -a "reload" -d "restart project(s)"
complete -c ky -n "__fish_use_subcommand" -a "kill" -d "kill process(es)"
complete -c ky -n "__fish_use_subcommand" -a "echo" -d "view logs"
complete -c ky -n "__fish_use_subcommand" -a "connect" -d "connect to process"
complete -c ky -n "__fish_use_subcommand" -a "restart" -d "restart process(es)"
complete -c ky -n "__fish_use_subcommand" -a "quit" -d "quit daemon"
complete -c ky -n "__fish_use_subcommand" -a "run" -d "run command"
complete -c ky -n "__fish_use_subcommand" -a "init" -d "create config file"
complete -c ky -n "__fish_use_subcommand" -a "add" -d "add a project"
complete -c ky -n "__fish_seen_subcommand_from add" -l "run" -d "register a standalone command" -r
complete -c ky -n "__fish_use_subcommand" -a "serve" -d "start web UI"
complete -c ky -n "__fish_use_subcommand" -a "ui" -d "start web UI (alias)"
complete -c ky -n "__fish_use_subcommand" -a "self" -d "self-management commands"
complete -c ky -n "__fish_use_subcommand" -a "help" -d "show help"
complete -c ky -n "__fish_use_subcommand" -a "version" -d "show version"

complete -c ky -n "__fish_seen_subcommand_from self" -a "update" -d "update to latest version"

complete -c ky -n "__fish_use_subcommand" -a "(__ky_projects)"

complete -c ky -n "__fish_seen_subcommand_from status st start stop reload kill echo connect restart quit run" -a "(__ky_projects)"
complete -c ky -n "__fish_seen_subcommand_from status st start stop reload kill echo connect restart quit run" -l all -d "target all projects"
complete -c ky -n "__fish_seen_subcommand_from status st start stop reload kill echo connect restart quit run" -s a -d "target all projects"
complete -c ky -n "__fish_seen_subcommand_from start stop restart reload" -l no-watch -d "skip post-command status watch"
complete -c ky -n "__fish_seen_subcommand_from start stop restart reload" -s W -d "skip post-command status watch"

complete -c ky -n "__fish_seen_subcommand_from serve ui" -l daemon -d "run in background"
complete -c ky -n "__fish_seen_subcommand_from serve ui" -s d -d "run in background"
complete -c ky -n "__fish_seen_subcommand_from serve ui" -l stop -d "stop daemon"
complete -c ky -n "__fish_seen_subcommand_from serve ui" -l echo -d "view daemon logs"
complete -c ky -n "__fish_seen_subcommand_from serve ui" -l restart -d "restart daemon"
complete -c ky -n "__fish_seen_subcommand_from serve ui" -l status -d "show daemon status"

complete -c ky -s h -l help -d "show help"
complete -c ky -s V -l version -d "show version"

complete -c kagaya -n "__fish_use_subcommand" -a "status st start stop reload kill echo connect restart quit run init add serve ui self help version"
complete -c kagaya -n "__fish_use_subcommand" -a "(__ky_projects)"
complete -c kagaya -n "__fish_seen_subcommand_from status st start stop reload kill echo connect restart quit run" -a "(__ky_projects)"
complete -c kagaya -n "__fish_seen_subcommand_from status st start stop reload kill echo connect restart quit run" -l all -d "target all projects"
