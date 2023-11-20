package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
	"gopkg.in/yaml.v3"
	v1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/rest"
	"k8s.io/client-go/tools/clientcmd"
)

type nsOptions struct {
	configAccess clientcmd.ConfigAccess
	out          io.Writer

	ns string
}

func Ns(out io.Writer, configAccess clientcmd.ConfigAccess) *cobra.Command {
	opts := &nsOptions{configAccess: configAccess, out: out}

	cmd := &cobra.Command{
		Use:   "ns [NAME]",
		Short: "Switch to a namespace",

		Args: cobra.MaximumNArgs(1),

		ValidArgsFunction: completeNamespaceFunc,

		RunE: func(_ *cobra.Command, args []string) error {
			if len(args) >= 1 {
				opts.ns = args[0]
			}
			return opts.run()
		},
	}

	return cmd
}

func (o *nsOptions) run() error {
	config, err := o.configAccess.GetStartingConfig()
	if err != nil {
		return err
	}

	ns, err := o.selectNs(config.CurrentContext)
	if err != nil {
		return err
	}

	ctx, ok := config.Contexts[config.CurrentContext]
	if !ok {
		return fmt.Errorf("Cannot find context %q", config.CurrentContext)
	}
	lastNs := ctx.Namespace
	changed := lastNs != ns
	ctx.Namespace = ns

	err = clientcmd.ModifyConfig(o.configAccess, *config, true)
	if err != nil {
		return fmt.Errorf("Update config: %w", err)
	}
	if changed {
		err = o.saveLast(lastNs)
		if err != nil {
			return fmt.Errorf("Save last ns: %w", err)
		}
	}

	fmt.Fprintf(o.out, "Switch to namespace %s\n", nameColor().Sprint(ns))
	return nil
}

func (o *nsOptions) selectNs(name string) (string, error) {
	if o.ns != "" {
		ns := o.ns
		if ns == "-" {
			var err error
			ns, err = o.readLast()
			if err != nil {
				return "", fmt.Errorf("Read last ns: %w", err)
			}
			if ns == "" {
				return "", errors.New("You have not switch to any namespace yet")
			}
		}
		return ns, nil
	}
	alias, err := o.readAlias()
	if err != nil {
		return "", err
	}

	var items []string
	for prefix, nsList := range alias {
		if strings.HasPrefix(name, prefix) {
			items = nsList
			break
		}
	}
	if len(items) == 0 {
		filename := o.configAccess.GetDefaultFilename()
		var restConfig *rest.Config
		restConfig, err = clientcmd.BuildConfigFromFlags("", filename)
		if err != nil {
			return "", err
		}

		var client *kubernetes.Clientset
		client, err = kubernetes.NewForConfig(restConfig)
		if err != nil {
			return "", fmt.Errorf("Init kube client: %w", err)
		}

		ctx := context.Background()
		var nsList *v1.NamespaceList
		nsList, err = client.CoreV1().Namespaces().List(ctx, metav1.ListOptions{})
		if err != nil {
			return "", fmt.Errorf("Get namespaces from server: %w", err)
		}
		items = make([]string, len(nsList.Items))
		for i, ns := range nsList.Items {
			items[i] = ns.Name
		}
	}

	if len(items) == 0 {
		return "", errors.New("No namespace to use")
	}

	idx, err := searchFzf(items)
	if err != nil {
		return "", err
	}

	return items[idx], nil
}

func (o *nsOptions) readAlias() (map[string][]string, error) {
	return readNsAlias(o.configAccess)
}

func readNsAlias(configAccess clientcmd.ConfigAccess) (map[string][]string, error) {
	filename := configAccess.GetDefaultFilename()
	dir := filepath.Dir(filename)
	aliasPath := filepath.Join(dir, "ns_alias.yaml")

	file, err := os.Open(aliasPath)
	if err != nil {
		if os.IsNotExist(err) {
			return make(map[string][]string), nil
		}
		return nil, fmt.Errorf("Open alias file: %w", err)
	}
	defer file.Close()

	decoder := yaml.NewDecoder(file)
	alias := make(map[string][]string, 0)
	err = decoder.Decode(&alias)
	if err != nil {
		return nil, fmt.Errorf("Decode alias file: %w", err)
	}

	return alias, nil
}

func (o *nsOptions) saveLast(name string) error {
	path := o.getLastPath()
	return os.WriteFile(path, []byte(name), 0644)
}

func (o *nsOptions) readLast() (string, error) {
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

func (o *nsOptions) getLastPath() string {
	filename := o.configAccess.GetDefaultFilename()
	dir := filepath.Dir(filename)
	return filepath.Join(dir, ".last_switch_ns")
}
