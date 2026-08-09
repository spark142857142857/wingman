use wingman_lib::grep_pattern::{GrepPatternErrorV1, GrepPatternV1};

fn compiled(pattern: &str) -> GrepPatternV1 {
    GrepPatternV1::compile(pattern, false, false).expect("compile P0 grep pattern")
}

#[test]
fn literals_any_star_and_anchors_match_complete_unicode_records() {
    assert!(compiled("TODO").is_match("prefix TODO suffix"));
    assert!(compiled("한.글").is_match("한글글"));
    assert!(compiled("ab*c").is_match("ac"));
    assert!(compiled("ab*c").is_match("abbbc"));
    assert!(compiled("^TODO$").is_match("TODO"));
    assert!(!compiled("^TODO$").is_match(" TODO"));
    assert!(!compiled("^TODO$").is_match("TODO later"));
    assert!(compiled("").is_match("anything"));
}

#[test]
fn classes_ranges_negation_and_escaped_class_members_are_exact() {
    assert!(compiled("^[abc][a-z][^0-9]$").is_match("az!"));
    assert!(!compiled("^[abc][a-z][^0-9]$").is_match("dz!"));
    assert!(compiled(r"^[\]\-\^\\]$").is_match("]"));
    assert!(compiled(r"^[\]\-\^\\]$").is_match("-"));
    assert!(compiled(r"^[\]\-\^\\]$").is_match("^"));
    assert!(compiled(r"^[\]\-\^\\]$").is_match("\\"));
    assert!(compiled("^[-a]$").is_match("-"));
    assert!(compiled("^[a-]$").is_match("-"));
}

#[test]
fn fixed_strings_disable_pattern_interpretation() {
    let pattern = GrepPatternV1::compile(r"C:\temp\[file].*", true, false).unwrap();
    assert!(pattern.is_match(r"path C:\temp\[file].* suffix"));
    assert!(!pattern.is_match(r"C:\temp\xfiley.txt"));
}

#[test]
fn ignore_case_uses_locale_independent_unicode_simple_folding() {
    let pattern = GrepPatternV1::compile("kσß", false, true).unwrap();
    assert!(pattern.is_match("KΣẞ"));
    assert!(GrepPatternV1::compile("i", false, true)
        .unwrap()
        .is_match("I"));
}

#[test]
fn unsupported_or_malformed_pattern_grammar_is_rejected() {
    for pattern in [
        "*a", "a**", "^*", "a^", "$a", "a$b", "[", "[]", "[^]", "[z-a]", "[a--b]", r"\q", r"abc\",
        "(a)", "a|b", "a+", "a?", "a{2}",
    ] {
        assert_eq!(
            GrepPatternV1::compile(pattern, false, false).err(),
            Some(GrepPatternErrorV1::InvalidSyntax),
            "pattern: {pattern:?}"
        );
    }
}

#[test]
fn fixed_mode_accepts_every_pattern_scalar_literally() {
    for pattern in ["*", r"\q", "(a|b)+?{2}", "[z-a]", "^"] {
        let compiled = GrepPatternV1::compile(pattern, true, false).unwrap();
        assert!(compiled.is_match(&format!("before{pattern}after")));
    }
}
