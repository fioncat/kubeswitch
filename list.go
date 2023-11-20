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

	wide bool
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

	cmd.Flags().BoolVarP(&opts.wide, "wide", "w", false, "Show more info")

	return cmd
}

func (o *listOption) run() error {
	config, err := o.configAccess.GetStartingConfig()
	if err != nil {
		return err
	}
	if len(config.Contexts) == 0 {
		return errors.New("No cluster to show")
	}

	rows := make([][]string, 0, len(config.Contexts))
	for name, ctx := range config.Contexts {
		if name == config.CurrentContext {
			name = fmt.Sprintf("* %s", name)
		} else {
			name = fmt.Sprintf("  %s", name)
		}

		row := []string{
			name,
			"  " + ctx.Namespace,
		}
		if o.wide {
			cluster, ok := config.Clusters[ctx.Cluster]
			if ok {
				row = append(row, "  "+cluster.Server)
			} else {
				row = append(row, "  ")
			}
		}

		rows = append(rows, row)
	}
	sort.Slice(rows, func(i, j int) bool {
		return rows[i][0] < rows[j][0]
	})

	fmt.Fprint(o.out, "  ")
	titles := []string{"name", "namespace"}
	if o.wide {
		titles = append(titles, "server")
	}
	ShowTable(o.out, titles, rows)
	return nil
}
