package corpus.build

import groovy.transform.ToString

@ToString(includeNames = true)
class Task {
    String name
    List<String> dependsOn = []
    Closure action = { -> }
}

trait Logged {
    List<String> log = []

    void note(String message) {
        log << "[${new Date().format('HH:mm')}] $message"
    }
}

class Build implements Logged {
    private final Map<String, Task> tasks = [:]

    Task task(String name, Map options = [:], Closure body = null) {
        List<String> deps = options.dependsOn ?: []
        def t = new Task(name: name, dependsOn: deps)
        if (body) {
            t.action = body
        }
        tasks[name] = t
        return t
    }

    List<String> order(String target, Set<String> seen = [] as Set) {
        if (!seen.add(target)) {
            return []
        }
        def deps = tasks[target]?.dependsOn ?: []
        return deps.collectMany { String d -> order(d, seen) } + [target]
    }

    void run(String target) {
        order(target).each { String n ->
            note "running $n"
            tasks[n]?.action?.call()
        }
    }
}

def build = new Build()
build.task('compile') { println 'compiling' }
build.task('test', dependsOn: ['compile']) { println "testing ${'∑'}" }
build.run('test')
assert build.order('test') == ['compile', 'test']
