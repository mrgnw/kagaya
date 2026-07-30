function __ky_services
	set -l config (set -q XDG_CONFIG_HOME; and echo $XDG_CONFIG_HOME; or echo $HOME/.config)/kagaya/projects.toml
	test -f $config; or return
	awk -F'[]=[]' '/^[a-zA-Z0-9_-]+[ \t]*=/ {gsub(/[ \t]/,"",$1); print $1} /^\[[a-zA-Z0-9_-]+\]/ {print $2}' $config
end

set -l verbs status st start stop restart logs echo show add remove rm init autostart reload-config rc serve self all help version

complete -c ky -f
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "status" -d "show service status"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "start" -d "start service(s)"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "stop" -d "stop service(s)"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "restart" -d "restart service(s)"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "logs" -d "show log file paths"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "echo" -d "tail + stream live output"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "show" -d "show service config"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "add" -d "register a service"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "remove" -d "unregister a service"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "init" -d "create config files"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "autostart" -d "start service(s) on login"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "reload-config" -d "re-sync plists from config"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "serve" -d "web UI daemon"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "self" -d "self-management"
complete -c ky -n "not __fish_seen_subcommand_from $verbs" -a "(__ky_services)" -d "service"

complete -c ky -n "__fish_seen_subcommand_from start stop restart status st logs echo show remove rm autostart" -a "(__ky_services)" -d "service"
complete -c ky -n "__fish_seen_subcommand_from start" -l wait -d "block until ready"
complete -c ky -n "__fish_seen_subcommand_from start restart" -l force -d "kill foreign port holders"
complete -c ky -n "__fish_seen_subcommand_from start" -l autostart -d "only autostart services"
complete -c ky -n "__fish_seen_subcommand_from start stop restart" -l all -d "all services"
complete -c ky -n "__fish_seen_subcommand_from serve" -a "stop restart status daemon foreground"
complete -c ky -n "__fish_seen_subcommand_from autostart" -a "on off"
complete -c ky -n "__fish_seen_subcommand_from self" -a "update"
