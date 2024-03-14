package main

import (
	"flag"
	"io"
	"log"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strings"

	"github.com/goccy/go-yaml"
)

var (
	addr = flag.String("addr", "127.0.0.1:8080", "server address")
)

func main() {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /convert", func(w http.ResponseWriter, r *http.Request) {
		url := r.URL.Query().Get("url")

		res, err := http.Get(url)
		if err != nil {
			panic(err)
		}

		body, err := io.ReadAll(res.Body)
		if err != nil {
			panic(err)
		}
		defer res.Body.Close()

		var data map[string]interface{}
		err = yaml.Unmarshal(body, &data)
		if err != nil {
			panic(err)
		}

		rules := data["rules"].([]any)
		for i, rule := range rules {
			rule := rule.(string)
			rules[i] = strings.ReplaceAll(rule, ",,", ",")
		}

		config, err := yaml.Marshal(data)
		if err != nil {
			panic(err)
		}

		w.Header().Set("Content-Disposition", "attachment; filename=config.yaml")
		w.Header().Set("Content-Type", "application/x-yaml")
		w.Write(config)
	})

	signalChan := make(chan os.Signal, 1)
	signal.Notify(signalChan, os.Interrupt)

	go func() {
		slog.Info("Server is running", "addr", *addr)
		if err := http.ListenAndServe(*addr, mux); err != nil {
			log.Fatal(err)
		}
	}()

	<-signalChan
	slog.Info("Shutting down...")
}
