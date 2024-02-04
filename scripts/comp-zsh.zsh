_kubeswitch() {
	local cmd=${words[1]}
    local cmp_args=("${words[@]:1}")
	local items=($($cmd --comp -- "${cmp_args[@]}" 2>/tmp/.kubeswitch_comp_logs))
	_describe 'command' items
}

compdef _kubeswitch kubeswitch
