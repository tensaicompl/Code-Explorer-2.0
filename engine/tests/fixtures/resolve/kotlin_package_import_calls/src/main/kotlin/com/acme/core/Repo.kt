package com.acme.core

class Repo(private val name: String) {
    fun find(id: Int): String = name + id

    fun count(): Int = name.length
}

fun makeRepo(name: String): Repo = Repo(name)
