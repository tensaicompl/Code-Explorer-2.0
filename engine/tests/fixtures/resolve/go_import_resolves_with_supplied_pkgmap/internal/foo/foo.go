package foo

// Greeting says hello.
func Greeting(name string) string {
	return "hello " + name
}

type Store struct{}

func (s *Store) Get(key string) string {
	return key
}

func NewStore() *Store {
	return &Store{}
}
