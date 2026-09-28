#include "util.h"
int util_add(int a, int b) { return a + b; }
int point_norm(const struct point *p) { return util_add(p->x * p->x, p->y * p->y); }
