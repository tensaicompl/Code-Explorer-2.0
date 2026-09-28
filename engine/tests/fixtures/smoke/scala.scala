package demo

class Greeter {
  def greet(name: String): String = "hi " + name
}

object Main {
  def main(args: Array[String]): Unit = println(new Greeter().greet("x"))
}
