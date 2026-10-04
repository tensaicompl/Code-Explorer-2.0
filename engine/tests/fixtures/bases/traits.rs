use std::fmt;

pub trait Shape {
    fn area(&self) -> f64;

    fn describe(&self) -> String {
        String::from("a shape")
    }
}

pub trait Named {}

pub struct Square {
    side: f64,
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

impl Named for Square {}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.describe())
    }
}

impl From<f64> for Square {
    fn from(side: f64) -> Self {
        Square { side }
    }
}
