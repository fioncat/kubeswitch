package main

import (
	"errors"
	"fmt"
	"io"
	"sort"

	"github.com/spf13/cobra"
	"k8s.io/client-go/tools/clientcmd"
)

type listOption struct {
	configAccess clientcmd.ConfigAccess
	out          io.Writer
}

func List(out io.Writer, configAccess clientcmd.ConfigAccess) *cobra.Command {
	opts := &listOption{configAccess: configAccess, out: out}

	cmd := &cobra.Command{
		Use:   "list",
		Short: "List clusters",

		Args: cobra.ExactArgs(0),

		ValidArgsFunction: completeContextFunc,

		RunE: func(_ *cobra.Command, _ []string) error {
			return opts.run()
		},
	}

	return cmd
}

func (o *listOption) run() error {
	config, err := o.configAccess.GetStartingConfig()
	if err != nil {
		return err
	}
	if len(config.Clusters) == 0 {
		return errors.New("No cluster to show")
	}

	rows := make([][]string, 0, len(config.Clusters))
	for name, cluster := range config.Clusters {
		if name == config.CurrentContext {
			name = fmt.Sprintf("* %s", name)
		} else {
			name = fmt.Sprintf("  %s", name)
		}

		rows = append(rows, []string{
			name,
			"  " + cluster.Server,
		})
	}
	sort.Slice(rows, func(i, j int) bool {
		return rows[i][0] < rows[j][0]
	})

	fmt.Fprint(o.out, "  ")
	ShowTable(o.out, []string{"name", "server"}, rows)
	return nil
}
