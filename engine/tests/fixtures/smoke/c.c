#include <stdio.h>

struct greeter { const char *prefix; };

static void greet(const struct greeter *g, const char *name) {
    printf("%s%s\n", g->prefix, name);
}

int main(void) {
    struct greeter g = {"hi "};
    greet(&g, "x");
    return 0;
}
