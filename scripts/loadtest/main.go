// Command loadtest sends a fixed number of requests at a target and reports
// latency and status distribution. It is a small, dependency-free probe for
// the demo stack, not a replacement for a real load generator.
package main

import (
	"bytes"
	"flag"
	"fmt"
	"io"
	"math/rand"
	"net/http"
	"os"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

type result struct {
	status  int
	latency time.Duration
	err     bool
	path    string
}

func main() {
	base := flag.String("base", "http://127.0.0.1:8080", "API or web base URL")
	pathsFlag := flag.String("paths", "/api/products,/api/categories,/api/plugins,/api/storefront/theme", "comma-separated paths")
	workers := flag.Int("c", 50, "concurrent workers")
	total := flag.Int("n", 5000, "total requests")
	duration := flag.Duration("d", 0, "run for this duration instead of a fixed request count")
	timeout := flag.Duration("timeout", 15*time.Second, "per-request timeout")
	headersFlag := flag.String("headers", "", "extra headers as Key: Value|Key: Value")
	spreadIPs := flag.Bool("spread-ips", false, "send a rotating X-Forwarded-For (API must trust the proxy)")
	ipCount := flag.Int("ips", 2000, "number of rotating client addresses")
	expectStatus := flag.Int("expect", 0, "count only this status as success (0 means any 2xx)")
	flag.Parse()

	paths := splitList(*pathsFlag)
	if len(paths) == 0 {
		fmt.Fprintln(os.Stderr, "at least one path is required")
		os.Exit(2)
	}
	headers := parseHeaders(*headersFlag)

	client := &http.Client{
		Timeout: *timeout,
		Transport: &http.Transport{
			MaxIdleConns:        *workers * 2,
			MaxIdleConnsPerHost: *workers * 2,
			MaxConnsPerHost:     *workers * 2,
			IdleConnTimeout:     30 * time.Second,
		},
	}

	var issued atomic.Int64
	var finished atomic.Int64
	results := make(chan result, *workers)
	stop := make(chan struct{})
	start := time.Now()

	var wg sync.WaitGroup
	for worker := 0; worker < *workers; worker++ {
		wg.Add(1)
		go func(worker int) {
			defer wg.Done()
			random := rand.New(rand.NewSource(time.Now().UnixNano() + int64(worker)))
			for {
				select {
				case <-stop:
					return
				default:
				}
				var index int64
				if *duration > 0 {
					index = issued.Add(1) - 1
				} else {
					index = issued.Add(1) - 1
					if index >= int64(*total) {
						return
					}
				}
				path := paths[random.Intn(len(paths))]
				request, err := http.NewRequest(http.MethodGet, strings.TrimRight(*base, "/")+path, nil)
				if err != nil {
					results <- result{err: true, path: path}
					finished.Add(1)
					continue
				}
				request.Header.Set("Accept", "application/json")
				request.Header.Set("User-Agent", "AzaharLoadTest/1.0")
				for key, value := range headers {
					request.Header.Set(key, value)
				}
				if *spreadIPs {
					ip := fmt.Sprintf("10.%d.%d.%d",
						random.Intn(250)+1, random.Intn(250)+1, random.Intn(*ipCount)%250)
					request.Header.Set("X-Forwarded-For", ip)
				}
				begin := time.Now()
				response, err := client.Do(request)
				elapsed := time.Since(begin)
				if err != nil {
					results <- result{latency: elapsed, err: true, path: path}
				} else {
					_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, 1<<20))
					response.Body.Close()
					results <- result{status: response.StatusCode, latency: elapsed, path: path}
				}
				finished.Add(1)
				if *duration > 0 && time.Since(start) >= *duration {
					return
				}
			}
		}(worker)
	}

	if *duration > 0 {
		go func() {
			time.Sleep(*duration)
			close(stop)
		}()
	}

	go func() {
		wg.Wait()
		close(results)
	}()

	statusCounts := make(map[int]int)
	pathCounts := make(map[string]int)
	latencies := make([]time.Duration, 0, max(1, *total))
	var errors int
	var successes int
	for item := range results {
		if item.err {
			errors++
			continue
		}
		statusCounts[item.status]++
		pathCounts[item.path]++
		latencies = append(latencies, item.latency)
		ok := item.status >= 200 && item.status < 300
		if *expectStatus != 0 {
			ok = item.status == *expectStatus
		}
		if ok {
			successes++
		}
	}
	took := time.Since(start)

	fmt.Printf("target        %s\n", *base)
	fmt.Printf("workers       %d\n", *workers)
	fmt.Printf("requests      %d in %s\n", finished.Load(), took.Round(time.Millisecond))
	fmt.Printf("throughput    %.0f req/s\n", float64(finished.Load())/took.Seconds())
	fmt.Printf("success       %d (%.3f%%)\n", successes, percent(successes, int(finished.Load())))
	fmt.Printf("errors        %d (%.3f%%)\n", errors, percent(errors, int(finished.Load())))
	fmt.Println("status        ", formatStatuses(statusCounts))
	if len(pathCounts) > 1 {
		fmt.Println("paths         ", formatPaths(pathCounts))
	}
	if len(latencies) > 0 {
		sort.Slice(latencies, func(i, j int) bool { return latencies[i] < latencies[j] })
		fmt.Printf("latency p50   %s\n", percentile(latencies, 50))
		fmt.Printf("latency p95   %s\n", percentile(latencies, 95))
		fmt.Printf("latency p99   %s\n", percentile(latencies, 99))
		fmt.Printf("latency max   %s\n", latencies[len(latencies)-1].Round(time.Millisecond))
	}
	if successes == int(finished.Load()) {
		fmt.Println("verdict       pass")
	} else {
		fmt.Println("verdict       fail")
		os.Exit(1)
	}
}

func splitList(value string) []string {
	parts := strings.Split(value, ",")
	out := make([]string, 0, len(parts))
	for _, part := range parts {
		part = strings.TrimSpace(part)
		if part != "" {
			out = append(out, part)
		}
	}
	return out
}

func parseHeaders(value string) map[string]string {
	out := make(map[string]string)
	for _, pair := range strings.Split(value, "|") {
		pair = strings.TrimSpace(pair)
		if pair == "" {
			continue
		}
		key, rest, found := strings.Cut(pair, ":")
		if !found {
			continue
		}
		key = strings.TrimSpace(key)
		if key == "" {
			continue
		}
		out[key] = strings.TrimSpace(rest)
	}
	return out
}

func percent(part, total int) float64 {
	if total == 0 {
		return 0
	}
	return float64(part) * 100 / float64(total)
}

func percentile(values []time.Duration, p int) time.Duration {
	if len(values) == 0 {
		return 0
	}
	index := (len(values) - 1) * p / 100
	return values[index].Round(time.Millisecond)
}

func formatStatuses(values map[int]int) string {
	keys := make([]int, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Ints(keys)
	var buffer bytes.Buffer
	for _, key := range keys {
		fmt.Fprintf(&buffer, "%d=%d ", key, values[key])
	}
	return strings.TrimSpace(buffer.String())
}

func formatPaths(values map[string]int) string {
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	var buffer bytes.Buffer
	for _, key := range keys {
		fmt.Fprintf(&buffer, "%s=%d ", key, values[key])
	}
	return strings.TrimSpace(buffer.String())
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
