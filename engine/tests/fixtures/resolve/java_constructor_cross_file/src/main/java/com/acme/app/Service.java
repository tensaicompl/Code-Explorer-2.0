package com.acme.app;

import com.acme.core.Repo;

public class Service {
    private final Repo repo = new Repo("s");

    public String run() {
        Repo local = new Repo("x");
        String a = local.find(1);
        String b = this.repo.find(2);
        return a + b + Repo.create().find(3);
    }
}
