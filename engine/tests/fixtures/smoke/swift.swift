struct Greeter {
    func greet(_ name: String) -> String {
        return "hi " + name
    }
}

func make() -> Greeter {
    return Greeter()
}
