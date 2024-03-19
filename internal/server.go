package internal

import (
	"io"
	"log"
	"log/slog"
	"net/http"
	"os"
	"os/signal"

	"github.com/dop251/goja"
	"github.com/goccy/go-yaml"
)

var (
	vm       = goja.New()
	mainFunc func(map[string]any) (map[string]any, error)
)

func initJsEngine(script string) {
	scriptContent, err := os.ReadFile(script)
	if err != nil {
		panic(err)
	}

	_, err = vm.RunString(string(scriptContent))
	if err != nil {
		panic(err)
	}

	err = vm.ExportTo(vm.Get("main"), &mainFunc)
	if err != nil {
		panic(err)
	}
}

func Run(addr string, token string, script string) {
	initJsEngine(script)

	mux := http.NewServeMux()

	mux.HandleFunc("GET /convert", tokenMiddleware(token, func(w http.ResponseWriter, r *http.Request) {
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

		if script != "" {
			data, err = mainFunc(data)
			if err != nil {
				panic(err)
			}
		}

		config, err := yaml.Marshal(data)
		if err != nil {
			panic(err)
		}

		w.Header().Set("Content-Disposition", "attachment; filename=config.yaml")
		w.Header().Set("Content-Type", "application/x-yaml")
		w.Write(config)
	}))

	signalChan := make(chan os.Signal, 1)
	signal.Notify(signalChan, os.Interrupt)

	go func() {
		slog.Info("Server is running", "addr", addr)
		if err := http.ListenAndServe(addr, mux); err != nil {
			log.Fatal(err)
		}
	}()

	<-signalChan
	slog.Info("Shutting down...")
}

func tokenMiddleware(token string, next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		queryToken := r.URL.Query().Get("token")
		if queryToken != token {
			http.Error(w, "Unauthorized", http.StatusUnauthorized)
			return
		}

		next(w, r)
	}
}
