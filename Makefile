.PHONY: build
build:
	@CGO_ENABLED=0 go build -o ./bin/kubeswitch

.PHONY: install
install: build
	@mkdir -p ${HOME}/go/bin
	@cp ./bin/kubeswitch ${HOME}/go/bin/kubeswitch
