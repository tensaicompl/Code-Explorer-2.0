package main

import (
	"fmt"

	"example.com/acme/internal/foo"
)

func main() {
	s := foo.NewStore()
	fmt.Println(foo.Greeting(s.Get("x")))
}
