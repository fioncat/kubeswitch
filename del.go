package main

import (
	"fmt"
	"io"

	"github.com/spf13/cobra"
	"k8s.io/client-go/tools/clientcmd"
)

type delOptions struct {
	configAccess clientcmd.ConfigAccess
	out          io.Writer

	name string
}

func Del(out io.Writer, configAccess clientcmd.ConfigAccess) *cobra.Command {
	opts := &delOptions{configAccess: configAccess, out: out}

	cmd := &cobra.Command{
		Use:   "del NAME",
		Short: "Delete a cluster",

		Args: cobra.ExactArgs(1),

		ValidArgsFunction: completeContextFunc,

		RunE: func(_ *cobra.Command, args []string) error {
			opts.name = args[0]
			return opts.run()
		},
	}

	return cmd
}

func (o *delOptions) run() error {
	config, err := o.configAccess.GetStartingConfig()
	if err != nil {
		return err
	}

	delete(config.Contexts, o.name)
	delete(config.AuthInfos, o.name)
	delete(config.Clusters, o.name)
	if o.name == config.CurrentContext {
		config.CurrentContext = ""
	}

	err = clientcmd.ModifyConfig(o.configAccess, *config, true)
	if err != nil {
		return fmt.Errorf("Modify config: %w", err)
	}
	fmt.Fprintf(o.out, "Delete cluster %q\n", o.name)

	return nil
}
