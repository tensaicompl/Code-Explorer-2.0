package demo

type Greeter struct{ Prefix string }

func (g *Greeter) Greet(name string) string {
	return g.Prefix + name
}

func Make() *Greeter {
	return &Greeter{Prefix: "hi "}
}
