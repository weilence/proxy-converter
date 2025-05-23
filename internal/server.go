package internal

import (
	"log"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
)

func Run(addr string, token string, configPath string) {
	mux := http.NewServeMux()

	mux.HandleFunc("GET /config", tokenMiddleware(token, func(w http.ResponseWriter, r *http.Request) {
		content, err := os.ReadFile(configPath)
		if err != nil {
			slog.Error("Failed to read config", "error", err)
			http.Error(w, "Internal Server Error", http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Disposition", "attachment; filename=config.yaml")
		w.Header().Set("Content-Type", "application/x-yaml")
		w.Write(content)
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
