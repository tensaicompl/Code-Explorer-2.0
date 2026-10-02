#include <stdio.h>

#define BROKEN(x) do { printf("%d\n", (x)); while (0)

int fine(void) { return 1; }

int broken(int a {
    BROKEN(a);
    return a +;
}

int after(void) { return fine() + 1; }
