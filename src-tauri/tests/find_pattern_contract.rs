use wingman_lib::find_pattern::{FindPatternErrorV1, FindPatternV1, MAX_FIND_PATTERN_BYTES};

#[test]
fn wildcard_pattern_matches_the_complete_unicode_basename() {
    let pattern = FindPatternV1::compile("*테스트?.[tT][sS]", false).unwrap();
    assert!(pattern.is_match("내테스트1.ts"));
    assert!(!pattern.is_match("내테스트.ts"));
    assert!(!pattern.is_match("prefix내테스트1.ts.bak"));
}

#[test]
fn wildcard_literals_and_escapes_do_not_gain_regex_meanings() {
    for value in ["^", "$", "+", "(", ")", "{", "}", "|"] {
        assert!(FindPatternV1::compile(value, false)
            .unwrap()
            .is_match(value));
    }
    assert!(FindPatternV1::compile(r"file\?.txt", false)
        .unwrap()
        .is_match("file?.txt"));
    assert!(FindPatternV1::compile("", false).unwrap().is_match(""));
}

#[test]
fn iname_uses_locale_independent_unicode_case_matching() {
    let pattern = FindPatternV1::compile("*Δ.TXT", true).unwrap();
    assert!(pattern.is_match("alphaδ.txt"));
    assert!(!pattern.is_match("alphaδ.txt.bak"));
}

#[test]
fn invalid_classes_escapes_separators_and_bounds_are_rejected() {
    for pattern in [r"[z-a]", r"[]", r"[abc", r"\q", r"trail\", "a/b", "a:b"] {
        assert_eq!(
            FindPatternV1::compile(pattern, false).err(),
            Some(FindPatternErrorV1::InvalidSyntax),
            "pattern: {pattern}"
        );
    }
    assert_eq!(
        FindPatternV1::compile(&"x".repeat(MAX_FIND_PATTERN_BYTES + 1), false).err(),
        Some(FindPatternErrorV1::InvalidSyntax)
    );
}
