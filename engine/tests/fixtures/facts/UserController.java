package com.acme.web;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestMethod;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/api")
public class UserController {
    @GetMapping("/users")
    public String list(int page, String filter) {
        return String.valueOf(page) + filter;
    }

    @GetMapping({"/members", "/people"})
    public String members() {
        return "";
    }

    @RequestMapping(path = {"/a", "/b"}, method = {RequestMethod.GET, RequestMethod.POST})
    public String both() {
        return "";
    }
}
