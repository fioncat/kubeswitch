package main

import (
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"

	"github.com/spf13/cobra"
	"k8s.io/client-go/tools/clientcmd"
	clientcmdapi "k8s.io/client-go/tools/clientcmd/api"
)

type mergeOptions struct {
	configAccess clientcmd.ConfigAccess
	out          io.Writer

	name     string
	filename string
}

func Merge(out io.Writer, configAccess clientcmd.ConfigAccess) *cobra.Command {
	opts := &mergeOptions{configAccess: configAccess, out: out}

	cmd := &cobra.Command{
		Use:   "merge [-f filename] [NAME]",
		Short: "Merge kube config",

		RunE: func(_ *cobra.Command, args []string) error {
			if len(args) >= 1 {
				opts.name = args[0]
			}
			return opts.run()
		},
	}

	flags := cmd.Flags()
	flags.StringVarP(&opts.filename, "file", "f", "", "The merge config filename, if not provided, will open an editor to edit config")

	return cmd
}

func (o *mergeOptions) run() error {
	config, err := o.configAccess.GetStartingConfig()
	if err != nil {
		return err
	}

	mergeConfig, err := o.getMergeConfig()
	if err != nil {
		return err
	}

	for name, cluster := range mergeConfig.Clusters {
		if o.name != "" {
			name = o.name
		}
		config.Clusters[name] = cluster
	}
	for name, authInfo := range mergeConfig.AuthInfos {
		if o.name != "" {
			name = o.name
		}
		config.AuthInfos[name] = authInfo
	}
	for name, ctx := range mergeConfig.Contexts {
		if o.name != "" {
			config.Contexts[o.name] = &clientcmdapi.Context{
				Cluster:  o.name,
				AuthInfo: o.name,
			}
			break
		}
		config.Contexts[name] = ctx
	}

	err = clientcmd.ModifyConfig(o.configAccess, *config, true)
	if err != nil {
		return fmt.Errorf("Modify kube config: %w", err)
	}

	return nil
}

func (o *mergeOptions) getMergeConfig() (*clientcmdapi.Config, error) {
	if o.filename != "" {
		mergeConfig, err := clientcmd.LoadFromFile(o.filename)
		if err != nil {
			return nil, fmt.Errorf("Load merge config from file: %w", err)
		}
		return mergeConfig, nil
	}

	data, err := o.edit()
	if err != nil {
		return nil, err
	}

	mergeConfig, err := clientcmd.Load(data)
	if err != nil {
		return nil, fmt.Errorf("Load merge config from edit: %w", err)
	}

	return mergeConfig, nil
}

func (o *mergeOptions) edit() ([]byte, error) {
	editor := os.Getenv("EDITOR")
	if editor == "" {
		return nil, errors.New("Missing env EDITOR to edit file")
	}
	fmt.Fprintf(o.out, "Use editor %q to edit kube config content.\n", editor)

	file, err := os.CreateTemp("", "edit-kubeconfig-*.yaml")
	if err != nil {
		return nil, fmt.Errorf("Create temp file: %w", err)
	}
	path := file.Name()
	abs, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}

	err = file.Close()
	if err != nil {
		return nil, fmt.Errorf("Close temp file: %w", err)
	}

	cmd := exec.Command(editor, abs)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	err = cmd.Run()
	if err != nil {
		return nil, fmt.Errorf("Use editor %q to edit temp file failed: %w", editor, err)
	}

	return os.ReadFile(abs)
}
