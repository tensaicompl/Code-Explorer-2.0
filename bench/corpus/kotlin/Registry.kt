package corpus.registry

import kotlin.properties.Delegates

annotation class Audited(val reason: String = "")

interface Clock {
    fun now(): Long
}

class FixedClock(private val at: Long) : Clock {
    override fun now() = at
}

class Registry(clock: Clock) : Clock by clock {
    private val listeners = mutableListOf<(String) -> Unit>()

    var name: String by Delegates.observable("unnamed") { _, old, new ->
        listeners.forEach { it("$old -> $new") }
    }

    @Audited(reason = "every registration is logged")
    fun register(entry: String): Boolean {
        if (entry.isBlank()) return false
        listeners.forEach { listener -> listener(entry) }
        return true
    }

    fun onChange(listener: (String) -> Unit) = listeners.add(listener)

    tailrec fun countDown(n: Int, acc: Int = 0): Int = if (n <= 0) acc else countDown(n - 1, acc + n)
}

enum class Level(val weight: Int) {
    LOW(1), HIGH(10);

    fun heavier(other: Level) = weight > other.weight
}
