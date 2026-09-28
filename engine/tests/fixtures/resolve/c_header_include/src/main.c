#include "../include/util.h"
int main(void) {
    struct point p = {3, 4};
    return util_add(point_norm(&p), 1);
}
