fn helper() -> u32 {
    2
}

#[test]
fn second_case() {
    let value = helper(); // A0: duplicate_symbol -> tests.second.helper
    assert_eq!(value, 2);
}
