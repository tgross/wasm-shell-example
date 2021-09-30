MAKEFLAGS += --warn-undefined-variables
SHELL = /usr/bin/env bash -o nounset -o errexit -o pipefail
.DEFAULT_GOAL = build

## display this help message
help:
	@echo -e "\033[32m"
	@echo "wasm-shell-example"
	@echo
	@awk '/^##.*$$/,/[a-zA-Z_-]+:/' $(MAKEFILE_LIST) | awk '!(NR%2){print $$0p}{p=$$0}' | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-16s\033[0m %s\n", $$1, $$2}' | sort

#-----------------------------------------
# build/test

.PHONY: build
## build the project
build: shell

#-----------------------------------------
# shell

SHELL_TARGET_DIR ?= shell/target/wasm32-wasi/debug
SHELL_TARGETS = $(shell find shell/src -name '*.rs') shell/Cargo.toml

.PHONY: shell
## build the embeddable echo shell
shell: $(SHELL_TARGET_DIR)/shell.wasm

$(SHELL_TARGET_DIR)/shell.wasm: $(SHELL_TARGETS)
	cd shell && cargo wasi build
