#include "matrix.hpp"

#include <algorithm>
#include <map>
#include <string>
#include <vector>

#define CORPUS_STRINGIFY(x) #x
#define CORPUS_CONCAT(a, b) a##b

namespace corpus::linalg {
namespace {

template <typename... Shapes>
std::vector<std::unique_ptr<Shape>> make_all(Shapes &&...shapes) {
    std::vector<std::unique_ptr<Shape>> out;
    (out.push_back(std::make_unique<std::decay_t<Shapes>>(std::forward<Shapes>(shapes))), ...);
    return out;
}

}  // namespace

double total_area(const std::vector<std::unique_ptr<Shape>> &shapes) {
    double total = 0;
    std::for_each(shapes.begin(), shapes.end(), [&total](const auto &s) { total += s->area(); });
    return total;
}

std::map<std::string, int> histogram(const std::vector<std::string> &words) {
    std::map<std::string, int> counts;
    for (const auto &[i, w] : std::vector<std::pair<int, std::string>>{{0, "un"}, {1, "deux"}}) {
        counts[w] += i;
    }
    for (const auto &w : words) {
        ++counts[w];
    }
    return counts;
}

int demo() {
    Matrix<int, 2, 3> a(1);
    Matrix<int, 3, 2> b(2);
    auto c = a * b;
    auto shapes = make_all(Circle(1.0), Circle(2.0));
    const char *raw = R"(raw "string" with \n kept)";
    int CORPUS_CONCAT(local, _value) = c(0, 0);
    try {
        if (total_area(shapes) < 0) throw std::runtime_error(CORPUS_STRINGIFY(negative area));
    } catch (const std::exception &e) {
        return -1;
    }
    return local_value + static_cast<int>(std::string(raw).size());
}

}  // namespace corpus::linalg
