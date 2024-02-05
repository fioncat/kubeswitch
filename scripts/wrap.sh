__kubeswitch_cmd() {
	if output=$(__wrap_cmd $@); then
		if [[ -z $output ]]; then
			return
		fi
		local items=( $(echo $output) )

		local header=${items[@]:0:1}
		if [[ $header != "__switch__" ]]; then
			echo "$output"
			return
		fi

		local cmd=${items[@]:1:1}
		local export_kubeconfig=${items[@]:2:1}
		local clean_flag=${items[@]:3:1}
		if [[ $clean_flag == "1" ]]; then
			unset KUBESWITCH_CONFIG KUBESWITCH_NAMESPACE KUBESWITCH_DISPLAY
			if [[ $export_kubeconfig == "1" ]]; then
				unset KUBECONFIG
			fi
			unalias ${cmd}
			return
		fi

		export KUBESWITCH_CONFIG="${items[@]:4:1}"
		export KUBESWITCH_NAMESPACE="${items[@]:5:1}"
		export KUBESWITCH_DISPLAY="${items[@]:6:1}"

		local kubectl_cmd="${items[@]:7:1}"
		local kubeconfig_path="${items[@]:8:1}"

		alias ${cmd}="${kubectl_cmd} --kubeconfig ${kubeconfig_path} --namespace ${KUBESWITCH_NAMESPACE}"
		if [[ $export_kubeconfig == "1" ]]; then
			export KUBECONFIG="${kubeconfig_path}"
		fi

		return
	fi
	return 1
}
