//! Unit tests for syntax section classification and cluster partitioning.

use super::*;
use crate::parse::{AddLine, CtxLine, DelLine, Hunk, LineNum, Row};

#[test]
fn test_classify_syntax_section() {
    assert_eq!(
        classify_syntax_section("/// Doc comment", false),
        SyntaxSectionKind::DocComment
    );
    assert_eq!(
        classify_syntax_section("// Helper line comment", false),
        SyntaxSectionKind::LineComment
    );
    assert_eq!(
        classify_syntax_section("# Python comment", true),
        SyntaxSectionKind::LineComment
    );
    assert_eq!(
        classify_syntax_section("#[derive(Debug, Clone)]", false),
        SyntaxSectionKind::Attribute
    );
    assert_eq!(
        classify_syntax_section("pub fn execute_job(&self) -> Result<()> {", false),
        SyntaxSectionKind::Declaration
    );
    assert_eq!(
        classify_syntax_section("let mut total = 0;", false),
        SyntaxSectionKind::Code
    );
    assert_eq!(
        classify_syntax_section("   ", false),
        SyntaxSectionKind::Blank
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
    let h = Hunk {
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 5,
        section: String::new(),
        rows: vec![
            Row::Del(DelLine::new(LineNum::new(1).unwrap(), "/// Obsolete doc")),
            Row::Ctx(CtxLine::new(
                LineNum::new(2).unwrap(),
                LineNum::new(2).unwrap(),
                "pub fn existing() {",
            )),
            Row::Add(AddLine::new(LineNum::new(3).unwrap(), "    let x = 10;")),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let segments = partition_hunk_segments(&h, false);
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
