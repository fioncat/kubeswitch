package main

import "fmt"

type notFoundError struct {
	name string
}

func (e *notFoundError) Error() string {
	return fmt.Sprintf("Cluster %q is not found", e.name)
}
