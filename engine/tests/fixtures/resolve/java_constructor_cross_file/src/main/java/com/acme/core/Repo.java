package com.acme.core;

public class Repo {
    private final String name;

    public Repo(String name) {
        this.name = name;
    }

    public String find(int id) {
        return name + id;
    }

    public static Repo create() {
        return new Repo("r");
    }
}
