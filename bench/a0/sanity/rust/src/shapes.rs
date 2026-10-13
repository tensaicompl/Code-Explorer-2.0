pub struct Widget;

impl Widget {
    pub fn new() -> Self {
        Widget
    }

    pub fn render(&self) -> &'static str {
        "widget"
    }
}

pub struct Alpha;
pub struct Beta;

impl Alpha {
    pub fn draw(&self) -> &'static str {
        "alpha"
    }
}

impl Beta {
    pub fn draw(&self) -> &'static str {
        "beta"
    }
}

pub trait Shape {
    fn area(&self) -> u32;
}

impl Shape for Alpha {
    fn area(&self) -> u32 {
        1
    }
}

impl Shape for Beta {
    fn area(&self) -> u32 {
        2
    }
}
