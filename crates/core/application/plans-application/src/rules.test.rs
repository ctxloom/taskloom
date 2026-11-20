//! The lenient reads: the frontmatter's title and sessions, a title, a name.

use super::*;

#[test]
fn frontmatter_reads_a_flow_list_quoted_values_and_needs_its_close() {
    let flow = "---\ntitle: 'It''s'\nsessions: [a, \"b\"]\n---\n";
    assert_eq!(
        frontmatter(flow),
        Frontmatter {
            title: "It's".into(),
            sessions: vec!["a".into(), "b".into()],
        }
    );
    assert_eq!(
        frontmatter("---\ntitle: open\n"),
        Frontmatter::default(),
        "no closing ---"
    );
    let after = "---\nsessions:\n  - a\nother: x\n- b\n---\n";
    assert_eq!(frontmatter(after).sessions, vec!["a"]);
}

#[test]
fn a_title_is_the_frontmatters_then_the_headings_then_the_name() {
    assert_eq!(title_of(Some("Front"), "# Head\n", "name"), "Front");
    assert_eq!(title_of(None, "intro\n# Head \n", "name"), "Head");
    assert_eq!(title_of(Some(" "), "# Head\n", "name"), "Head");
    assert_eq!(title_of(None, "no heading\n#\n", "name"), "name");
}

#[test]
fn a_plan_name_drops_the_suffix_and_md_too_and_a_bare_name_is_itself() {
    assert_eq!(plan_name("/tree/docs/notes.plan.md"), "notes");
    assert_eq!(plan_name("docs/notes.md"), "notes");
    assert_eq!(plan_name("README"), "README");
}
