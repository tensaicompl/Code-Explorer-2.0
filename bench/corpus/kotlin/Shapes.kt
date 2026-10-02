package corpus.shapes

import kotlin.math.PI
import kotlin.math.sqrt

/** Sealed hierarchies, data classes, extensions, generics and lambdas. */
sealed class Shape {
    abstract fun area(): Double

    data class Circle(val radius: Double) : Shape() {
        override fun area() = PI * radius * radius
    }

    data class Rect(val width: Double, val height: Double) : Shape() {
        override fun area(): Double = width * height
    }

    object Empty : Shape() {
        override fun area() = 0.0
    }
}

fun Shape.describe(): String = when (this) {
    is Shape.Circle -> "circle r=$radius, area=${"%.2f".format(area())}"
    is Shape.Rect -> "rect ${width}x$height"
    Shape.Empty -> "empty"
}

interface Repository<K, V : Any> {
    fun get(key: K): V?
    fun put(key: K, value: V)
}

class MemoryRepository<K, V : Any> : Repository<K, V> {
    private val store = mutableMapOf<K, V>()
    override fun get(key: K): V? = store[key]
    override fun put(key: K, value: V) {
        store[key] = value
    }

    companion object {
        fun <K, V : Any> of(vararg pairs: Pair<K, V>): MemoryRepository<K, V> =
            MemoryRepository<K, V>().apply { pairs.forEach { (k, v) -> put(k, v) } }
    }
}

inline fun <reified T> List<Any>.only(): List<T> = filterIsInstance<T>()

fun hypotenuse(a: Double, b: Double) = sqrt(a * a + b * b)

val banner = """
    |Formes — résumé
    |  ${Shape.Circle(1.0).describe()}
""".trimMargin()

fun main() {
    val repo = MemoryRepository.of("a" to Shape.Circle(2.0), "b" to Shape.Rect(1.0, 3.0))
    val shapes = listOf(repo.get("a"), repo.get("b"), Shape.Empty).filterNotNull()
    shapes.sortedBy { it.area() }.forEach { println(it.describe()) }
    println(listOf<Any>(1, "two", 3.0).only<String>())
    println(hypotenuse(3.0, 4.0))
}
