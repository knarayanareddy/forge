#[path = "../src/inject01.rs"]
mod inject01;

#[test]
fn inject01_zero_escapes() {
    inject01::test_inject01_impl().expect("INJECT-01");
}
