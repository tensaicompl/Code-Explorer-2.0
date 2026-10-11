<?php

namespace Acme\Core;

class Repo
{
    private string $name;

    public function __construct(string $name)
    {
        $this->name = $name;
    }

    public function find(int $id): string
    {
        return $this->name . $id;
    }
}

function make_repo(string $name): Repo
{
    return new Repo($name);
}
