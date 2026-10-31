//! The rules beyond the reviewed ones: where a path is placed, the text a
//! new plan gets, and the frontmatter it reads back.

use super::*;

#[test]
fn locate_places_a_path_inside_or_outside_the_work_root() {
    let at = |given| locate("/tree", given);
    assert_eq!(
        at("docs/x.plan.md"),
        Located::Inside("docs/x.plan.md".into())
    );
    assert_eq!(at("/tree/./a/../b.md"), Located::Inside("b.md".into()));
    assert_eq!(
        at("/home/b/x.plan.md"),
        Located::Outside("/home/b/x.plan.md".into())
    );
    assert_eq!(at("../x.plan.md"), Located::Outside("/x.plan.md".into()));
    assert_eq!(at("/tree"), Located::Outside("/tree".into()));
    assert_eq!(at(""), Located::Outside("/tree".into()));
    assert_eq!(
        locate("/tree/", "/tree/x.plan.md"),
        Located::Inside("x.plan.md".into())
    );
}

#[test]
fn resolve_path_refuses_the_work_root_itself_as_no_plan_file() {
    assert_eq!(
        resolve_path("/tree", "/tree"),
        Err(PathError::OutsideWorkRoot("/tree".into()))
    );
    assert_eq!(
        resolve_path("/tree", "docs/x.plan.md.bak"),
        Err(PathError::NotAPlanFile("docs/x.plan.md.bak".into()))
    );
}

#[test]
fn a_new_plans_text_reads_back_its_title_and_session() {
    for title in [
        "Plans move into taskloom",
        "Plans: phase 2",
        "\"quoted\" and \\ slashed",
        "- dash first",
        "ends with colon:",
        "hash # inside",
    ] {
        let text = new_plan_text(title, "inept-sixth-stick");
        assert_eq!(
            frontmatter(&text),
            Frontmatter {
                title: title.into(),
                sessions: vec!["inept-sixth-stick".into()],
            },
            "{text}"
        );
        assert!(text.ends_with(&format!("# {title}\n")), "{text}");
    }
    let text = new_plan_text("T", "");
    assert_eq!(text, "---\ntitle: T\n---\n\n# T\n");
}

#[test]
fn a_plain_title_is_written_plain() {
    assert_eq!(
        yaml_scalar("Plans move into taskloom"),
        "Plans move into taskloom"
    );
    assert_eq!(yaml_scalar("a: b"), "\"a: b\"");
    assert_eq!(yaml_scalar(""), "\"\"");
    assert_eq!(yaml_scalar(" padded"), "\" padded\"");
}

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
fn a_title_is_the_frontmatters_then_the_heading_then_the_name() {
    assert_eq!(
        title_of("---\ntitle: Front\n---\n# Head\n", "name"),
        "Front"
    );
    assert_eq!(title_of("intro\n# Head \n", "name"), "Head");
    assert_eq!(title_of("no heading\n#\n", "name"), "name");
}

#[test]
fn a_plan_name_drops_md_too_and_a_bare_name_is_itself() {
    assert_eq!(plan_name("docs/notes.md"), "notes");
    assert_eq!(plan_name("README"), "README");
    assert_eq!(joined("/tree/", "a/b.plan.md"), "/tree/a/b.plan.md");
}
