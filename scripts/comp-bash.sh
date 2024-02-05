__kubeswitch_comp() {
	local cmd=${COMP_WORDS[0]}
	local cmp_args=("${COMP_WORDS[@]:1}")
	local items=($($cmd --comp -- "${cmp_args[@]}" 2>>/tmp/.kubeswitch_comp_logs))

	COMPREPLY=($(compgen -W "${items[*]}" -- "${COMP_WORDS[COMP_CWORD]}"))
}

complete -o default -F __kubeswitch_comp __kubeswitch_cmd
