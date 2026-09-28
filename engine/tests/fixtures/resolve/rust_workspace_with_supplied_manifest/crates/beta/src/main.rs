fn main() {
    let m = alpha::Meter { v: 3 };
    let total = alpha::compute(m.read());
    println!("{}", total);
}
