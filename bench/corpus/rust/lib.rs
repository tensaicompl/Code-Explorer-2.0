//! Traits, generics, lifetimes, macros, enums and pattern matching.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub mod shapes {
    pub trait Shape: std::fmt::Debug {
        fn area(&self) -> f64;
        fn name(&self) -> &'static str {
            "shape"
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Circle {
        pub radius: f64,
    }

    impl Shape for Circle {
        fn area(&self) -> f64 {
            std::f64::consts::PI * self.radius * self.radius
        }
        fn name(&self) -> &'static str {
            "circle"
        }
    }
}

#[derive(Debug)]
pub enum Event<'a> {
    Created { id: u32, name: &'a str },
    Deleted(u32),
    Batch(Vec<Event<'a>>),
}

impl fmt::Display for Event<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Created { id, name } if name.is_empty() => write!(f, "created #{id}"),
            Event::Created { id, name } => write!(f, "created #{id} ({name})"),
            Event::Deleted(id) => write!(f, "deleted #{id}"),
            Event::Batch(events) => {
                for e in events {
                    writeln!(f, "{e}")?;
                }
                Ok(())
            }
        }
    }
}

macro_rules! counted {
    ($($name:ident => $value:expr),* $(,)?) => {{
        let mut map = HashMap::new();
        $( map.insert(stringify!($name), $value); )*
        map
    }};
}

pub struct Registry<K, V> {
    inner: Arc<Mutex<HashMap<K, V>>>,
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> Registry<K, V> {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.inner.lock().expect("poisoned").insert(key, value)
    }

    pub fn snapshot(&self) -> Vec<(K, V)> {
        let guard = self.inner.lock().expect("poisoned");
        guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> Default for Registry<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}

pub fn demo() -> usize {
    let counts = counted! { apples => 3, pears => 5 };
    let größe = counts.values().sum::<i32>() as usize;
    let raw = r#"raw "string" — kept"#;
    let unsafe_len = unsafe { raw.as_bytes().get_unchecked(0) };
    größe + *unsafe_len as usize + longest("a", "bc").len()
}
