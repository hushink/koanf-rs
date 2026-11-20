#[cfg(all(feature = "watch", feature = "http"))]
#[test]
fn watched_requires_watchable() {
    let test_cases = trybuild::TestCases::new();
    test_cases.compile_fail("tests/ui/watched_requires_watchable.rs");
}
