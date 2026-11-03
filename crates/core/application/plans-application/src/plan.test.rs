//! A plan read from its file, and what is derived across files.

use super::*;

fn parsed(path: &str, text: &str) -> Plan {
    Plan::parse(path, text)
}

#[test]
fn a_plan_has_its_files_metadata_and_the_defaults_for_what_it_lacks() {
    let plan = parsed(
        "/tree/docs/p.plan.md",
        "---\nid: quiet-amber-falcon\ntags: [area:cli]\nsessions: [one, two]\n---\nIntro\n\n# The Heading\n",
    );
    assert_eq!(plan.id.as_deref(), Some("quiet-amber-falcon"));
    assert_eq!(plan.name, "p");
    assert_eq!(
        plan.title, "The Heading",
        "the first heading, else the name"
    );
    assert_eq!(plan.written_title, None);
    assert_eq!(plan.status, "draft");
    assert_eq!(plan.written_status, None);
    assert_eq!(plan.session(), "one");
    assert_eq!(plan.sessions, ["one", "two"]);
    assert_eq!(plan.unreadable, None);
    let bare = parsed("/tree/bare.plan.md", "no heading, no frontmatter\n");
    assert_eq!(bare.title, "bare");
    assert_eq!(bare.id, None);
    assert_eq!(bare.session(), "");
}

#[test]
fn a_heading_inside_the_frontmatter_is_a_comment_not_the_title() {
    let plan = parsed("/t/a.plan.md", "---\n# a comment\nid: x-y\n---\nbody\n");
    assert_eq!(plan.title, "a");
}

#[test]
fn a_frontmatter_that_does_not_parse_still_lists_its_title_and_sessions() {
    let plan = parsed(
        "/t/b.plan.md",
        "---\ntitle: Kept\nsessions: [s]\nflavor: [unclosed\n---\n# B\n",
    );
    assert!(plan.unreadable.is_some());
    assert_eq!(plan.title, "Kept");
    assert_eq!(plan.sessions, ["s"]);
    assert_eq!(plan.id, None);
}

#[test]
fn the_metadata_is_as_the_file_writes_it() {
    let plan = parsed("/t/c.plan.md", "---\nid: a-b\nstatus: active\n---\n# C\n");
    let meta = plan.meta();
    assert_eq!(meta.title, None, "the heading is the body's, not metadata");
    assert_eq!(meta.status.as_deref(), Some("active"));
    assert_eq!(meta.id.as_deref(), Some("a-b"));
}

fn with(id: &str, extra: &str) -> Plan {
    parsed(
        &format!("/t/{id}.plan.md"),
        &format!("---\nid: {id}\n{extra}---\n# {id}\n"),
    )
}

#[test]
fn a_plan_another_supersedes_is_superseded_whatever_it_writes() {
    let mut plans = vec![
        with("old-one", "status: active\n"),
        with("new-one", "supersedes: [old-one]\n"),
        with("other-one", ""),
    ];
    derive(&mut plans);
    assert_eq!(plans[0].status, SUPERSEDED);
    assert_eq!(plans[0].written_status.as_deref(), Some("active"));
    assert_eq!(plans[0].superseded_by, ["new-one"]);
    assert_eq!(plans[1].status, "draft");
    assert_eq!(plans[2].status, "draft");
    assert!(plans[2].superseded_by.is_empty());
}

#[test]
fn a_plan_with_no_id_supersedes_and_is_superseded_by_no_one_and_a_self_supersede_counts_for_nothing(
) {
    let mut plans = vec![
        parsed("/t/noid.plan.md", "---\nsupersedes: [old-one]\n---\n# n\n"),
        with("old-one", ""),
        with("loop-one", "supersedes: [loop-one]\n"),
    ];
    derive(&mut plans);
    assert_eq!(
        plans[1].status, "draft",
        "a plan with no id supersedes nothing"
    );
    assert_eq!(plans[2].status, "draft", "itself does not count");
}
