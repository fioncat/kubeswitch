package main

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strings"

	"github.com/fatih/color"
)

func searchFzf(items []string) (int, error) {
	var inputBuf bytes.Buffer
	inputBuf.Grow(len(items))
	for _, item := range items {
		inputBuf.WriteString(item + "\n")
	}

	var outputBuf bytes.Buffer
	cmd := exec.Command("fzf")
	cmd.Stdin = &inputBuf
	cmd.Stderr = os.Stderr
	cmd.Stdout = &outputBuf

	err := cmd.Run()
	if err != nil {
		if os.IsNotExist(err) {
			return 0, errors.New("fzf has not been installed in your system, please install it first")
		}
		return 0, err
	}

	result := outputBuf.String()
	result = strings.TrimSpace(result)
	for idx, item := range items {
		if item == result {
			return idx, nil
		}
	}

	return 0, fmt.Errorf("cannot find %q from fzf result", result)
}

func nameColor() *color.Color {
	return color.New(color.Bold, color.FgGreen)

}
