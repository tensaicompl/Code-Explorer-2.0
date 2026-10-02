// Package server: generics, interfaces, goroutines, channels and closures.
package server

import (
	"context"
	"errors"
	"fmt"
	"os"
	"sort"
	"sync"
	"time"
)

// Number is a constraint for the numeric generics below.
type Number interface {
	~int | ~int64 | ~float64
}

func Sum[T Number](values ...T) T {
	var total T
	for _, v := range values {
		total += v
	}
	return total
}

type Store[K comparable, V any] struct {
	mu    sync.RWMutex
	items map[K]V
}

func NewStore[K comparable, V any]() *Store[K, V] {
	return &Store[K, V]{items: make(map[K]V)}
}

func (s *Store[K, V]) Put(key K, value V) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.items[key] = value
}

func (s *Store[K, V]) Get(key K) (V, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	v, ok := s.items[key]
	return v, ok
}

type Handler interface {
	Handle(ctx context.Context, job Job) error
}

type Job struct {
	ID       int
	Name     string `json:"name"`
	Attempts int
}

type HandlerFunc func(ctx context.Context, job Job) error

func (f HandlerFunc) Handle(ctx context.Context, job Job) error { return f(ctx, job) }

var ErrRetry = errors.New("retry")

type Server struct {
	handler Handler
	results chan error
	timeout time.Duration
}

func New(h Handler) *Server {
	timeout := 5 * time.Second
	if v := os.Getenv("SERVER_TIMEOUT"); v != "" {
		if d, err := time.ParseDuration(v); err == nil {
			timeout = d
		}
	}
	return &Server{handler: h, results: make(chan error, 8), timeout: timeout}
}

func (s *Server) Run(ctx context.Context, jobs []Job) []error {
	ctx, cancel := context.WithTimeout(ctx, s.timeout)
	defer cancel()
	var wg sync.WaitGroup
	for _, job := range jobs {
		wg.Add(1)
		go func(j Job) {
			defer wg.Done()
			defer func() {
				if r := recover(); r != nil {
					s.results <- fmt.Errorf("job %d panicked: %v", j.ID, r)
				}
			}()
			s.results <- s.handler.Handle(ctx, j)
		}(job)
	}
	go func() { wg.Wait(); close(s.results) }()
	var errs []error
	for err := range s.results {
		if errors.Is(err, ErrRetry) {
			continue
		}
		if err != nil {
			errs = append(errs, err)
		}
	}
	sort.Slice(errs, func(i, j int) bool { return errs[i].Error() < errs[j].Error() })
	return errs
}

const banner = `server — ready
	raw string with "quotes" and \n kept literally`

type größe int
