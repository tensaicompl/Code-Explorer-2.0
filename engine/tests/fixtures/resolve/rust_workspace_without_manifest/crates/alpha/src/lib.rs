pub fn compute(x: i32) -> i32 {
    x * 2
}

pub struct Meter {
    pub v: i32,
}

impl Meter {
    pub fn read(&self) -> i32 {
        self.v
    }
}
