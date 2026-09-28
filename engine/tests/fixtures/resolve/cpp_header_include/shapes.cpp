#include "shapes.hpp"
namespace geo {
double total(const Shape &a, const Shape &b) { return a.area() + b.area(); }
}
