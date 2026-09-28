package demo

class Greeter(private val prefix: String) {
    fun greet(name: String): String = prefix + name
}

fun main() {
    println(Greeter("hi ").greet("x"))
}
