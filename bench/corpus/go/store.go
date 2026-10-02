package server

import "strings"

type Shape interface {
	Area() float64
	Name() string
}

type Rect struct{ W, H float64 }
type Square struct {
	Rect
	label string
}

func (r Rect) Area() float64   { return r.W * r.H }
func (r Rect) Name() string     { return "rect" }
func (s Square) Name() string   { return strings.ToUpper(s.label) }
func Describe(s Shape) string   { return s.Name() }
func Largest(shapes ...Shape) (best Shape) {
	for _, s := range shapes {
		switch v := s.(type) {
		case Square:
			if best == nil || v.Area() > best.Area() {
				best = v
			}
		default:
			if best == nil || s.Area() > best.Area() {
				best = s
			}
		}
	}
	return
}

func Matrix(n int) [][]int {
	m := make([][]int, n)
	for i := range m {
		m[i] = make([]int, n)
		for j := range m[i] {
			m[i][j] = i * j
		}
	}
	return m
}
