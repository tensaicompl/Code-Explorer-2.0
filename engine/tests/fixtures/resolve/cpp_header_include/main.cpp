#include "shapes.hpp"
int main() {
    geo::Square a(2.0);
    geo::Square b(3.0);
    double t = geo::total(a, b);
    return a.area() > t ? 1 : 0;
}
