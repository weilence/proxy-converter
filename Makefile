NAME = proxy-converter

.PHONY: build

build:
	@echo "Building the project"
	@go build -o $(NAME) main.go