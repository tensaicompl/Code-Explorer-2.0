package com.acme.app;
import com.acme.core.Repo;
public class Service {
  public String run() {
    Repo r = Repo.create();
    return r.find(1);
  }
}
