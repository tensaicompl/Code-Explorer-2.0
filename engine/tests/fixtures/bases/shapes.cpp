namespace geo {
class Shape {};
class Circle : public Shape, protected Named {
  public:
    double area() const { return 1.0; }
};
}
