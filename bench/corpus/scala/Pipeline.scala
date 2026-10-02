package corpus.pipeline

import scala.collection.mutable
import scala.util.{Failure, Success, Try}

/** Traits, case classes, pattern matching and for-comprehensions. */
sealed trait Stage[-I, +O] {
  def run(input: I): Try[O]
}

final case class Map1[I, O](f: I => O) extends Stage[I, O] {
  def run(input: I): Try[O] = Try(f(input))
}

final case class Filter[I](p: I => Boolean) extends Stage[I, Option[I]] {
  def run(input: I): Try[Option[I]] = Success(if (p(input)) Some(input) else None)
}

object Pipeline {
  private val log = mutable.ListBuffer.empty[String]

  def describe(value: Any): String = value match {
    case n: Int if n < 0          => s"negative $n"
    case n: Int                   => s"int $n"
    case (a, b)                   => s"pair ($a, $b)"
    case Some(inner)              => s"some ${describe(inner)}"
    case xs: List[_] if xs.isEmpty => "empty list"
    case head :: tail             => s"list starting ${describe(head)} and ${tail.size} more"
    case _                        => "something else"
  }

  def combine(xs: List[Int], ys: List[Int]): List[(Int, Int)] =
    for {
      x <- xs
      y <- ys
      if (x + y) % 2 == 0
    } yield (x, y)

  def runAll[A](value: A, stages: List[Stage[A, A]]): Either[String, A] =
    stages.foldLeft[Either[String, A]](Right(value)) {
      case (Right(v), stage) =>
        stage.run(v) match {
          case Success(next) => Right(next)
          case Failure(err)  => Left(err.getMessage)
        }
      case (left, _) => left
    }

  def main(args: Array[String]): Unit = {
    log += "démarrage"
    println(describe(List(1, 2, 3)))
    println(combine(List(1, 2), List(3, 4)))
    println(runAll(10, List(Map1[Int, Int](_ * 2), Map1[Int, Int](_ - 1))))
  }
}
