__kubeswitch_cmd() {
	if output=$(__wrap_cmd $@); then
		if [ -z $output ]; then
			return
		fi
		IFS=$'\n'; items=( $(echo -e "$output") );

		local header=${items[1]}
		if [[ $header != "__switch__" ]]; then
			echo $output
			return
		fi

		local cmd=${items[2]}
		local export_kubeconfig=${items[3]}
		local clean_flag=${items[4]}
		if [[ $clean_flag == "1" ]]; then
			unset KUBESWITCH_CONFIG KUBESWITCH_NAMESPACE KUBESWITCH_DISPLAY
			if [[ $export_kubeconfig == "1" ]]; then
				unset KUBECONFIG
			fi
			unalias ${cmd}
			return
		fi

		export KUBESWITCH_CONFIG="${items[5]}"
		export KUBESWITCH_NAMESPACE="${items[6]}"
		export KUBESWITCH_DISPLAY="${items[7]}"

		local kubectl_cmd="${items[8]}"
		local kubeconfig_path="${items[9]}"

		alias ${cmd}="${kubectl_cmd} --kubeconfig ${kubeconfig_path} --namespace ${KUBESWITCH_NAMESPACE}"
		if [[ $export_kubeconfig == "1" ]]; then
			export KUBECONFIG="${kubeconfig_path}"
		fi

		return
	fi
	return 1
}
