protocol Shape {
    var area: Double { get }
    func scaled(by factor: Double) -> Self
}

struct Circle: Shape {
    var radius: Double
    var area: Double { .pi * radius * radius }
    func scaled(by factor: Double) -> Circle { Circle(radius: radius * factor) }
}

struct Rect: Shape {
    var width, height: Double
    var area: Double { width * height }
    func scaled(by factor: Double) -> Rect { Rect(width: width * factor, height: height * factor) }
}

func largest<S: Shape>(_ shapes: [S]) -> S? {
    shapes.max { $0.area < $1.area }
}

@propertyWrapper
struct Clamped {
    private var value: Double
    let range: ClosedRange<Double>

    init(wrappedValue: Double, _ range: ClosedRange<Double>) {
        self.range = range
        self.value = min(max(wrappedValue, range.lowerBound), range.upperBound)
    }

    var wrappedValue: Double {
        get { value }
        set { value = min(max(newValue, range.lowerBound), range.upperBound) }
    }
}

struct Settings {
    @Clamped(0...1) var opacity: Double = 0.5
}
