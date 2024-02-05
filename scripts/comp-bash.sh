__kubeswitch_comp() {
	local words
	COMPREPLY=()

	# Call _init_completion from the bash-completion package
    # to prepare the arguments properly
    if declare -F _init_completion >/dev/null 2>&1; then
        _init_completion -n =: || return
    else
		# Macs have bash3 for which the bash-completion package doesn't include
		# _init_completion. This is a minimal version of that function.
		COMPREPLY=()
    	_get_comp_words_by_ref "$@" cur prev words cword
    fi


	local args=("${words[@]:1}")
    local comp_cmd="${words[0]} --comp -- ${args[*]}"

	COMPREPLY=($(eval "${comp_cmd}" 2>>/tmp/.kubeswitch_comp_logs))
}

complete -o default -F __kubeswitch_comp __kubeswitch_cmd
