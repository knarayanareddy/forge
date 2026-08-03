
#[path = "../src/skill03.rs"]
mod skill03;

#[test]
fn skill03_zero_escapes() {
    skill03::test_skill03_impl().expect("SKILL-03");
}
