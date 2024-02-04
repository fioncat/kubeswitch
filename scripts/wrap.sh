_kubeswitch() {
	if output=$(_wrap $@); then
		if [ -z $output ]; then
			return
		fi
		IFS=$'\n'; items=( $(echo -e "$output") );

		local cmd=${items[1]}
		local export_kubeconfig=${items[2]}
		local clean_flag=${items[3]}
		if [[ $clean_flag == "1" ]]; then
			unset KUBESWITCH_CONFIG KUBESWITCH_NAMESPACE KUBESWITCH_DISPLAY
			if [[ $export_kubeconfig == "1" ]]; then
				unset KUBECONFIG
			fi
			unalias ${cmd}
			return
		fi

		export KUBESWITCH_CONFIG="${items[4]}"
		export KUBESWITCH_NAMESPACE="${items[5]}"
		export KUBESWITCH_DISPLAY="${items[6]}"

		local kubectl_cmd="${items[7]}"
		local kubeconfig_path="${items[8]}"

		alias ${cmd}="${kubectl_cmd} --kubeconfig ${kubeconfig_path} --namespace ${KUBESWITCH_NAMESPACE}"
		if [[ $export_kubeconfig == "1" ]]; then
			export KUBECONFIG="${kubeconfig_path}"
		fi

		return
	fi
	return 1
}
