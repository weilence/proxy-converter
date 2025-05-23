package main

import (
	"flag"

	"github.com/weilence/proxy-converter/internal"
)

var (
	addr   = flag.String("addr", "127.0.0.1:8080", "server address")
	config = flag.String("config", "", "config")
	token  = flag.String("token", "", "token")
)

func main() {
	flag.Parse()

	internal.Run(*addr, *token, *config)
}
