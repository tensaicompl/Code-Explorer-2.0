#include <string>

namespace demo {
class Greeter {
public:
    std::string greet(const std::string &name) const { return "hi " + name; }
};
}

int main() {
    demo::Greeter g;
    return g.greet("x").size() > 0 ? 0 : 1;
}
