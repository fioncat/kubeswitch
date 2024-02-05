__kubeswitch_comp() {
	local comp_cmd="${words[1]} --comp -- ${words[2,-1]}"
	local items=($(eval ${comp_cmd} 2>>/tmp/.kubeswitch_comp_logs))
	_describe 'command' items
}

compdef __kubeswitch_comp __kubeswitch_cmd
