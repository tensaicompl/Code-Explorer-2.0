<?php

function slugify(string $text, string $sep = '-'): string
{
    $text = preg_replace('/[^\p{L}\p{N}]+/u', $sep, mb_strtolower($text));
    return trim($text, $sep);
}

function memoize(callable $fn): Closure
{
    static $cache = [];
    return function (...$args) use ($fn, &$cache) {
        $key = serialize($args);
        return $cache[$key] ??= $fn(...$args);
    };
}

$config = [
    'db' => ['host' => getenv('DB_HOST') ?: 'localhost', 'port' => (int) ($_ENV['DB_PORT'] ?? 5432)],
    'features' => ['search' => true, 'map' => false],
    'nested' => [[[[1, 2], [3]], []]],
];

$greet = static fn (string $name): string => "Bonjour, {$name}!";
echo $greet(slugify('Crème Brûlée')), PHP_EOL;
