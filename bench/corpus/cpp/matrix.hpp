#pragma once

#include <array>
#include <cstddef>
#include <functional>
#include <memory>
#include <ostream>
#include <stdexcept>
#include <type_traits>

namespace corpus::linalg {

template <typename T, std::size_t R, std::size_t C>
class Matrix {
    static_assert(std::is_arithmetic_v<T>, "numeric element type");

public:
    using value_type = T;

    constexpr Matrix() = default;
    explicit Matrix(T fill) { data_.fill(fill); }

    constexpr T &operator()(std::size_t r, std::size_t c) { return data_[r * C + c]; }
    constexpr const T &operator()(std::size_t r, std::size_t c) const { return data_[r * C + c]; }

    template <std::size_t K>
    Matrix<T, R, K> operator*(const Matrix<T, C, K> &rhs) const {
        Matrix<T, R, K> out;
        for (std::size_t i = 0; i < R; ++i)
            for (std::size_t j = 0; j < K; ++j)
                for (std::size_t k = 0; k < C; ++k)
                    out(i, j) += (*this)(i, k) * rhs(k, j);
        return out;
    }

    friend std::ostream &operator<<(std::ostream &os, const Matrix &m) {
        for (std::size_t i = 0; i < R * C; ++i) os << m.data_[i] << (i % C == C - 1 ? '\n' : ' ');
        return os;
    }

private:
    std::array<T, R * C> data_{};
};

struct Shape {
    virtual ~Shape() = default;
    virtual double area() const = 0;
    virtual const char *name() const { return "shape"; }
};

struct Circle final : Shape {
    explicit Circle(double r) : radius(r) {}
    double area() const override { return 3.14159 * radius * radius; }
    const char *name() const override { return "circle"; }
    double radius;
};

}  // namespace corpus::linalg
