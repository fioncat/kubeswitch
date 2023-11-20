package main

import (
	"bytes"
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

type setOptions struct {
	configAccess clientcmd.ConfigAccess
	out          io.Writer

	name     string
	filename string
}

func Set(out io.Writer, configAccess clientcmd.ConfigAccess) *cobra.Command {
	opts := &setOptions{configAccess: configAccess, out: out}

	cmd := &cobra.Command{
		Use:   "set [-f filename] NAME",
		Short: "Set cluster",

		Args: cobra.ExactArgs(1),

		ValidArgsFunction: completeContextFunc,

		RunE: func(cmd *cobra.Command, args []string) error {
			opts.name = args[0]
			if opts.name == "" {
				return cmd.Usage()
			}
			return opts.run()
		},
	}

	flags := cmd.Flags()
	flags.StringVarP(&opts.filename, "file", "f", "", "The merge config filename, if not provided, will open an editor to edit config")

	return cmd
}

func (o *setOptions) run() error {
	config, err := o.configAccess.GetStartingConfig()
	if err != nil {
		return err
	}

	configEdit := o.getConfigToEdit(config)
	newConfig, err := o.edit(configEdit)
	if err != nil {
		return err
	}

	if len(newConfig.Clusters) == 0 || len(newConfig.AuthInfos) == 0 {
		fmt.Fprintln(o.out, "None cluster, cancel set")
		return nil
	}
	if len(newConfig.Clusters) != 1 || len(newConfig.AuthInfos) != 1 {
		return errors.New("Invalid edit config, the number of cluster and user should be one")
	}

	var cluster *clientcmdapi.Cluster
	for _, c := range newConfig.Clusters {
		cluster = c
		break
	}

	var authInfo *clientcmdapi.AuthInfo
	for _, a := range newConfig.AuthInfos {
		authInfo = a
		break
	}

	ns := "default"
	if ctx, ok := config.Contexts[o.name]; ok {
		ns = ctx.Namespace
	}

	config.Clusters[o.name] = cluster
	config.AuthInfos[o.name] = authInfo
	config.Contexts[o.name] = &clientcmdapi.Context{
		Cluster:   o.name,
		AuthInfo:  o.name,
		Namespace: ns,
	}

	fmt.Fprintf(o.out, "Set cluster %q done.\n", o.name)
	err = clientcmd.ModifyConfig(o.configAccess, *config, true)
	if err != nil {
		return fmt.Errorf("Write config: %w", err)
	}

	return nil
}

func (o *setOptions) getConfigToEdit(cfg *clientcmdapi.Config) *clientcmdapi.Config {
	cluster, ok := cfg.Clusters[o.name]
	if !ok {
		return nil
	}
	ctx, ok := cfg.Contexts[o.name]
	if !ok {
		return nil
	}
	authInfo, ok := cfg.AuthInfos[o.name]
	if !ok {
		return nil
	}
	return &clientcmdapi.Config{
		Clusters: map[string]*clientcmdapi.Cluster{
			o.name: cluster,
		},
		Contexts: map[string]*clientcmdapi.Context{
			o.name: ctx,
		},
		AuthInfos: map[string]*clientcmdapi.AuthInfo{
			o.name: authInfo,
		},
	}
}

func (o *setOptions) edit(cfg *clientcmdapi.Config) (*clientcmdapi.Config, error) {
	editor := os.Getenv("EDITOR")
	if editor == "" {
		return nil, errors.New("Missing env EDITOR to edit file")
	}
	fmt.Fprintf(o.out, "Use editor %q to edit kube config content.\n", editor)

	var data []byte
	var err error
	if cfg != nil {
		data, err = clientcmd.Write(*cfg)
		if err != nil {
			return nil, fmt.Errorf("Encode kube config: %w", err)
		}
	}

	file, err := os.CreateTemp("", "edit-kubeconfig-*.yaml")
	if err != nil {
		return nil, fmt.Errorf("Create temp file: %w", err)
	}
	defer file.Close()

	path := file.Name()
	abs, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}

	if len(data) > 0 {
		buffer := bytes.NewBuffer(data)
		_, err = io.Copy(file, buffer)
		if err != nil {
			return nil, fmt.Errorf("Write temp file: %w", err)
		}
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

	data, err = os.ReadFile(abs)
	if err != nil {
		return nil, fmt.Errorf("Read temp file after editing: %w", err)
	}

	editedConfig, err := clientcmd.Load(data)
	if err != nil {
		return nil, fmt.Errorf("Load edited config: %w", err)
	}

	return editedConfig, nil
}
