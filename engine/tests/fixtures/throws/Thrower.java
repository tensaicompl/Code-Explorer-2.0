package acme;

import java.io.IOException;

public class Thrower {
    public void read(String path) throws IOException {
        if (path == null) {
            throw new IllegalArgumentException("path");
        }
    }
}
