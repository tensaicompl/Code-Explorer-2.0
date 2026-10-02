package corpus.shapes

trait Shape {
  def area: Double
  def name: String = getClass.getSimpleName
}

abstract class Polygon(val sides: Int) extends Shape

class Square(side: Double) extends Polygon(4) {
  override def area: Double = side * side
}

class Circle(radius: Double) extends Shape {
  override val area: Double = math.Pi * radius * radius
}

object Shapes {
  implicit class ShapeOps(val s: Shape) extends AnyVal {
    def larger(other: Shape): Shape = if (s.area >= other.area) s else other
  }

  def total(shapes: Seq[Shape]): Double = shapes.map(_.area).sum

  lazy val unit: Shape = new Square(1.0)

  def nested(depth: Int): Int = {
    def go(d: Int, acc: Int): Int = if (d == 0) acc else go(d - 1, acc + d)
    go(depth, 0)
  }
}
