package main

import (
	"errors"
	"fmt"
	"io"
	"os"

	"github.com/fatih/color"
	"github.com/spf13/cobra"
	"k8s.io/client-go/tools/clientcmd"
)

func Cmd(out io.Writer) *cobra.Command {
	patchOptions := clientcmd.NewDefaultPathOptions()

	cmd := &cobra.Command{
		Use:   "kubeswitch",
		Short: "Switch between different clusters",

		Args: cobra.ExactArgs(0),

		SilenceErrors: true,
		SilenceUsage:  true,

		RunE: func(_ *cobra.Command, _ []string) error {
			config, err := patchOptions.GetStartingConfig()
			if err != nil {
				return err
			}

			ctxName := config.CurrentContext
			if ctxName == "" {
				return errors.New("No context selected")
			}
			ctx, ok := config.Contexts[ctxName]
			if !ok {
				return fmt.Errorf("Cannot find context %q", ctxName)
			}
			ns := ctx.Namespace
			if ns == "" {
				ns = "default"
			}

			fmt.Fprintf(out, "Current cluster: %s\n", nameColor().Sprint(ctxName))
			fmt.Fprintf(out, "Current namespace: %s\n", nameColor().Sprint(ns))

			return nil
		},
	}

	cmd.AddCommand(Set(out, patchOptions))
	cmd.AddCommand(Use(out, patchOptions))
	cmd.AddCommand(Ns(out, patchOptions))
	cmd.AddCommand(Del(out, patchOptions))
	cmd.AddCommand(List(out, patchOptions))

	return cmd
}

func main() {
	out := os.Stderr
	cmd := Cmd(out)

	err := cmd.Execute()
	if err != nil {
		fmt.Fprintf(out, "%s: %v\n", color.RedString("error"), err)
		os.Exit(1)
	}
}
