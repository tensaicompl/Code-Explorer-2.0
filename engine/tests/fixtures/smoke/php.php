<?php
namespace Demo;

class Greeter {
    public function greet(string $name): string {
        return "hi " . $name;
    }
}

function make(): Greeter {
    return new Greeter();
}
