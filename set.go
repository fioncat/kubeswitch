package main

import (
	"io"

	"k8s.io/client-go/tools/clientcmd"
)

type setOptions struct {
	configAccess clientcmd.ConfigAccess
	out          io.Writer

	name     string
	filename string
}
