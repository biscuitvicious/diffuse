//! Unit tests for syntax section classification and cluster partitioning.

use diffuse_syntax::registry::lang_profile;

use super::*;
use crate::parse::{AddLine, CtxLine, DelLine, Hunk, Row};

#[test]
fn test_classify_syntax_section() {
    let rs = lang_profile("rs");
    let py = lang_profile("py");

    assert_eq!(
        classify_syntax_section("/// Doc comment", rs),
        SyntaxSectionKind::DocComment
    );
    assert_eq!(
        classify_syntax_section("// Helper line comment", rs),
        SyntaxSectionKind::LineComment
    );
    assert_eq!(
        classify_syntax_section("# Python comment", py),
        SyntaxSectionKind::LineComment
    );
    assert_eq!(
        classify_syntax_section("#[derive(Debug, Clone)]", rs),
        SyntaxSectionKind::Attribute
    );
    assert_eq!(
        classify_syntax_section("pub fn execute_job(&self) -> Result<()> {", rs),
        SyntaxSectionKind::Declaration
    );
    assert_eq!(
        classify_syntax_section("let mut total = 0;", rs),
        SyntaxSectionKind::Code
    );
    assert_eq!(classify_syntax_section("   ", rs), SyntaxSectionKind::Blank);
    // When profile is None, all non-blank lines classify as plain Code
    assert_eq!(
        classify_syntax_section("pub fn execute_job(&self) -> Result<()> {", None),
        SyntaxSectionKind::Code
    );
}

#[test]
fn test_syntax_section_compatibility() {
    assert!(SyntaxSectionKind::DocComment.is_compatible(SyntaxSectionKind::DocComment));
    assert!(SyntaxSectionKind::Code.is_compatible(SyntaxSectionKind::Code));
    assert!(SyntaxSectionKind::Code.is_compatible(SyntaxSectionKind::Declaration));
    assert!(!SyntaxSectionKind::DocComment.is_compatible(SyntaxSectionKind::Code));
    assert!(!SyntaxSectionKind::Attribute.is_compatible(SyntaxSectionKind::DocComment));
}

#[test]
fn test_partition_hunk_segments_with_syntax_barriers() {
    let rs = lang_profile("rs");
    let h = Hunk {
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 5,
        section: String::new(),
        rows: vec![
            Row::del(DelLine::new(1, "/// Obsolete doc")),
            Row::ctx(CtxLine::new(2, 2, "pub fn existing() {")),
            Row::add(AddLine::new(3, "    let x = 10;")),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let segments = partition_hunk_segments(&h, None, None, rs);
    assert_eq!(segments.len(), 3);

    // Segment 0: Doc comment deletion cluster
    match &segments[0] {
        HunkSegment::Cluster(c) => {
            assert_eq!(c.section, SyntaxSectionKind::DocComment);
            assert_eq!(c.old_rows.len(), 1);
            assert_eq!(c.new_rows.len(), 0);
        }
        _ => panic!("Expected cluster"),
    }

    // Segment 1: Context line
    match &segments[1] {
        HunkSegment::Context(r) => {
            assert_eq!(r.text(), "pub fn existing() {");
        }
        _ => panic!("Expected context"),
    }

    // Segment 2: Code addition cluster
    match &segments[2] {
        HunkSegment::Cluster(c) => {
            assert_eq!(c.section, SyntaxSectionKind::Code);
            assert_eq!(c.old_rows.len(), 0);
            assert_eq!(c.new_rows.len(), 1);
        }
        _ => panic!("Expected cluster"),
    }
}
