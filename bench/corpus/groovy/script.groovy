def config = [
    host : System.getenv('APP_HOST') ?: 'localhost',
    ports: (8080..8082).toList(),
    nested: [a: [b: [c: [1, [2, [3]]]]]],
]

def words = 'the quick brown fox'.tokenize()
def lengths = words.collectEntries { [(it): it.size()] }
def longest = words.max { it.size() }

def multiline = """\
Host: ${config.host}
Ports: ${config.ports.join(', ')}
Longest word: $longest
"""

switch (lengths.size()) {
    case 0: println 'none'; break
    case 1..3: println 'few'; break
    default: println "many: ${lengths}"
}

def matcher = 'sku-42' =~ /sku-(\d+)/
if (matcher.matches()) {
    println "number ${matcher[0][1]}"
}
