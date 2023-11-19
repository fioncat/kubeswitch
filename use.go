package main

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"

	"github.com/spf13/cobra"
	"k8s.io/client-go/tools/clientcmd"
	clientcmdapi "k8s.io/client-go/tools/clientcmd/api"
)

type useOptions struct {
	configAccess clientcmd.ConfigAccess
	out          io.Writer

	name string
}

func Use(out io.Writer, configAccess clientcmd.ConfigAccess) *cobra.Command {
	opts := &useOptions{configAccess: configAccess, out: out}

	cmd := &cobra.Command{
		Use:   "use [NAME]",
		Short: "Switch to a cluster",

		Args: cobra.MaximumNArgs(1),

		ValidArgsFunction: completeContextFunc,

		RunE: func(_ *cobra.Command, args []string) error {
			if len(args) >= 1 {
				opts.name = args[0]
			}
			return opts.run()
		},
	}

	return cmd
}

func (o *useOptions) run() error {
	config, err := o.configAccess.GetStartingConfig()
	if err != nil {
		return err
	}

	name, err := o.selectContext(config)
	if err != nil {
		return err
	}

	lastName := config.CurrentContext
	changed := lastName != name
	config.CurrentContext = name
	err = clientcmd.ModifyConfig(o.configAccess, *config, true)
	if err != nil {
		return fmt.Errorf("Modify config: %w", err)
	}
	if changed {
		err = o.saveLast(lastName)
		if err != nil {
			return fmt.Errorf("Save last use: %w", err)
		}
	}

	fmt.Fprintf(o.out, "Switch to cluster %s\n", nameColor().Sprint(name))
	return nil
}

func (o *useOptions) selectContext(config *clientcmdapi.Config) (string, error) {
	if o.name != "" {
		name := o.name
		if o.name == "-" {
			var err error
			name, err = o.readLast()
			if err != nil {
				return "", fmt.Errorf("Read last name: %w", err)
			}
			if name == "" {
				return "", errors.New("You have not switch to any cluster yet")
			}
		}
		if _, ok := config.Contexts[name]; !ok {
			return "", fmt.Errorf("Cannot find cluster %q", o.name)
		}

		return name, nil
	}

	names := make([]string, 0, len(config.Contexts))
	for name := range config.Contexts {
		names = append(names, name)
	}
	sort.Strings(names)

	idx, err := searchFzf(names)
	if err != nil {
		return "", fmt.Errorf("Search fzf: %w", err)
	}

	return names[idx], nil
}

func (o *useOptions) saveLast(name string) error {
	path := o.getLastPath()
	return os.WriteFile(path, []byte(name), 0644)
}

func (o *useOptions) readLast() (string, error) {
	path := o.getLastPath()
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return "", nil
		}
		return "", err
	}
	return string(data), nil
}

func (o *useOptions) getLastPath() string {
	filename := o.configAccess.GetDefaultFilename()
	dir := filepath.Dir(filename)
	return filepath.Join(dir, ".last_switch_cluster")
}
