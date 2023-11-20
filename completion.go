package main

import (
	"context"
	"sort"
	"strings"

	"github.com/spf13/cobra"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/tools/clientcmd"
)

func completeContextFunc(_ *cobra.Command, args []string, toComplete string) ([]string, cobra.ShellCompDirective) {
	if len(args) != 0 {
		return nil, cobra.ShellCompDirectiveNoFileComp
	}

	patchOptions := clientcmd.NewDefaultPathOptions()
	config, err := patchOptions.GetStartingConfig()
	if err != nil {
		return nil, cobra.ShellCompDirectiveNoFileComp
	}

	var ret []string
	for name := range config.Contexts {
		if strings.HasPrefix(name, toComplete) {
			ret = append(ret, name)
		}
	}
	sort.Strings(ret)

	return ret, cobra.ShellCompDirectiveNoFileComp
}

func completeNamespaceFunc(_ *cobra.Command, args []string, toComplete string) ([]string, cobra.ShellCompDirective) {
	if len(args) != 0 {
		return nil, cobra.ShellCompDirectiveNoFileComp
	}

	patchOptions := clientcmd.NewDefaultPathOptions()
	config, err := patchOptions.GetStartingConfig()
	if err != nil {
		return nil, cobra.ShellCompDirectiveNoFileComp
	}

	alias, err := readNsAlias(patchOptions)
	if err != nil {
		return nil, cobra.ShellCompDirectiveNoFileComp
	}

	var items []string
	for prefix, nsList := range alias {
		if strings.HasPrefix(config.CurrentContext, prefix) {
			items = nsList
			break
		}
	}
	if len(items) == 0 {
		filename := patchOptions.GetDefaultFilename()
		restConfig, err := clientcmd.BuildConfigFromFlags("", filename)
		if err != nil {
			return nil, cobra.ShellCompDirectiveNoFileComp
		}

		client, err := kubernetes.NewForConfig(restConfig)
		if err != nil {
			return nil, cobra.ShellCompDirectiveNoFileComp
		}

		ctx := context.Background()
		nsList, err := client.CoreV1().Namespaces().List(ctx, metav1.ListOptions{})
		if err != nil {
			return nil, cobra.ShellCompDirectiveNoFileComp
		}
		items = make([]string, len(nsList.Items))
		for i, ns := range nsList.Items {
			items[i] = ns.Name
		}
	}

	var ret []string
	for _, item := range items {
		if strings.HasPrefix(item, toComplete) {
			ret = append(ret, item)
		}
	}
	sort.Strings(ret)

	return ret, cobra.ShellCompDirectiveNoFileComp
}
