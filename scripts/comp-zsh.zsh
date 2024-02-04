_kubeswitch() {
	echo "xxxxxxxxxxxxxxxx" >> /tmp/comp-logs
	local items=($(./target/debug/kubeswitch --comp -- "${words[@]}" 2>/tmp/.kubeswitch_comp_logs))
	echo "${items[@]}" >> /tmp/comp-logs
	_describe 'command' items
}

compdef ./target/debug/kubeswitch _kubeswitch
