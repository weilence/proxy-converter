package internal

import (
	"io"
	"log"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strings"

	"github.com/goccy/go-yaml"
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

		var config map[string]any
		err = yaml.Unmarshal(content, &config)
		if err != nil {
			slog.Error("Failed to unmarshal config", "error", err)
			http.Error(w, "Internal Server Error", http.StatusInternalServerError)
			return
		}

		expand := r.URL.Query().Get("expand") == "1"
		if expand {
			proxyProviders, ok := config["proxy-providers"].(map[string]any)
			if ok {
				proxyProvidersCache := make(map[string][]string)
				for proxyProviderName, proxyProvider := range proxyProviders {
					proxyProviderMap, ok := proxyProvider.(map[string]any)
					if !ok {
						continue
					}

					proxyProviderType, ok := proxyProviderMap["type"].(string)
					if !ok {
						continue
					}

					if proxyProviderType != "http" {
						continue
					}

					proxyProviderUrl, ok := proxyProviderMap["url"].(string)
					if !ok {
						continue
					}

					proxyProviderContent, err := downloadFile(proxyProviderUrl)
					if err != nil {
						slog.Error("Failed to download file", "error", err)
						http.Error(w, "Internal Server Error", http.StatusInternalServerError)
						return
					}

					var proxyProviderConfig map[string]any
					err = yaml.Unmarshal(proxyProviderContent, &proxyProviderConfig)
					if err != nil {
						slog.Error("Failed to unmarshal proxy provider config", "error", err)
						http.Error(w, "Internal Server Error", http.StatusInternalServerError)
						return
					}

					proxies, ok := config["proxies"].([]any)
					if !ok {
						continue
					}

					downloadProxies, ok := proxyProviderConfig["proxies"].([]any)
					if !ok {
						continue
					}

					var excludeFilter string
					if excludeFilter, ok = proxyProviderMap["exclude-filter"].(string); !ok {
						excludeFilter = ""
					}

					for _, downloadProxy := range downloadProxies {
						downloadProxyMap, ok := downloadProxy.(map[string]any)
						if !ok {
							continue
						}

						downloadProxyName, ok := downloadProxyMap["name"].(string)
						if !ok {
							continue
						}

						if excludeFilter != "" && strings.Contains(downloadProxyName, excludeFilter) {
							continue
						}

						proxyProvidersCache[proxyProviderName] = append(proxyProvidersCache[proxyProviderName], downloadProxyName)
						proxies = append(proxies, downloadProxy)
					}

					config["proxies"] = proxies
				}

				delete(config, "proxy-providers")

				proxyGroups, ok := config["proxy-groups"].([]any)
				if ok {
					for _, proxyGroup := range proxyGroups {
						proxyGroupMap, ok := proxyGroup.(map[string]any)
						if !ok {
							continue
						}

						proxyGroupUse, ok := proxyGroupMap["use"].([]any)
						if !ok {
							continue
						}

						for _, proxyGroupName := range proxyGroupUse {
							if proxies, ok := proxyProvidersCache[proxyGroupName.(string)]; ok {
								ps, ok := proxyGroupMap["proxies"].([]any)
								if !ok {
									proxyGroupMap["proxies"] = proxies
								} else {
									for _, proxy := range proxies {
										ps = append(ps, proxy)
									}
									proxyGroupMap["proxies"] = ps
								}
							}
						}
						delete(proxyGroupMap, "use")
					}
				}
			}
		}

		bs, err := yaml.Marshal(config)
		if err != nil {
			slog.Error("Failed to marshal config", "error", err)
			http.Error(w, "Internal Server Error", http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Disposition", "attachment; filename=config.yaml")
		w.Header().Set("Content-Type", "application/x-yaml")
		w.Write(bs)
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

func downloadFile(url string) ([]byte, error) {
	resp, err := http.Get(url)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	return io.ReadAll(resp.Body)
}
