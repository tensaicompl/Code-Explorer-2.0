package demo

class Greeter {
    String greet(String name) {
        return "hi " + name
    }
}

def make() {
    return new Greeter()
}
