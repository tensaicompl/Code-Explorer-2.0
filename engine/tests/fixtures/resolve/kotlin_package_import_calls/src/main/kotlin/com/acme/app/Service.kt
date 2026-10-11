package com.acme.app

import com.acme.core.Repo
import com.acme.core.makeRepo

class Service {
    private val repo: Repo = makeRepo("users")

    fun lookup(id: Int): String {
        val found = repo.find(id)
        return found + repo.count()
    }

    fun direct(): String = Repo("direct").find(1)

    fun unknown(): Int = helper(3)
}
