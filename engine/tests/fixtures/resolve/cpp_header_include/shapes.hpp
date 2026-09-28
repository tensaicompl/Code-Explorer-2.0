#pragma once
namespace geo {
class Shape {
public:
    virtual ~Shape() {}
    virtual double area() const = 0;
};
class Square : public Shape {
public:
    explicit Square(double s) : side(s) {}
    double area() const override { return side * side; }
private:
    double side;
};
double total(const Shape &a, const Shape &b);
}
