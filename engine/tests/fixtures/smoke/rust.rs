pub struct Greeter {
    prefix: String,
}

impl Greeter {
    pub fn greet(&self, name: &str) -> String {
        format!("{}{}", self.prefix, name)
    }
}

pub fn make() -> Greeter {
    Greeter { prefix: "hi ".to_string() }
}
