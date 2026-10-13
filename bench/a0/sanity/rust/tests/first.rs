fn helper() -> u32 {
    1
}

#[test]
fn first_case() {
    assert_eq!(helper(), 1); // A0: duplicate_symbol -> tests.first.helper
}
