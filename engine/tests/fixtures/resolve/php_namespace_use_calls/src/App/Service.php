<?php

namespace Acme\App;

use Acme\Core\Repo;

use function Acme\Core\make_repo;

class Service
{
    private Repo $repo;

    public function __construct()
    {
        $this->repo = make_repo('users');
    }

    public function lookup(int $id): string
    {
        $direct = new Repo('direct');
        return $direct->find($id) . $this->repo->find($id);
    }

    public function missing(): void
    {
        undefined_helper(3);
    }
}
