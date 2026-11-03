//! The frontmatter: found between its fences, read, and edited in place.

use angzarr_yaml::Value;

use super::*;

const HAND: &str = "---\n# kept: a comment\nid: quiet-amber-falcon\ntitle: Original\n\nstatus: draft\nflavor: {a: 1, b: [x, y]}\ntags: [area:cli, priority=3]\nsessions:\n  - older\n---\n# Body\n\n---\nnot a fence\n";

#[test]
fn a_file_is_split_at_its_fences() {
    let Split::Fenced { open, front, rest } = split("---\na: 1\n---\n# B\n") else {
        panic!("fenced")
    };
    assert_eq!((open, front, rest), ("---\n", "a: 1\n", "---\n# B\n"));
    assert_eq!(split("# no fence\n"), Split::Absent);
    assert_eq!(split(""), Split::Absent);
    assert_eq!(split("---\na: 1\n"), Split::Unclosed);
    assert_eq!(
        split("--- x\na: 1\n---\n"),
        Split::Absent,
        "a fence is a line of its own"
    );
    assert!(matches!(
        split("---\r\na: 1\r\n---\r\nbody"),
        Split::Fenced { .. }
    ));
    assert!(
        matches!(split("---\na: 1\n...\nbody"), Split::Fenced { .. }),
        "dots close it"
    );
    assert!(
        matches!(split("---\n---\nbody"), Split::Fenced { front: "", .. }),
        "an empty frontmatter"
    );
}

#[test]
fn the_body_is_what_follows_the_closing_fence_line_or_the_whole_text() {
    assert_eq!(body("---\na: 1\n---\n# B\n"), "# B\n");
    assert_eq!(body("---\na: 1\n---"), "");
    assert_eq!(body("# B\n---\nx\n"), "# B\n---\nx\n");
    assert_eq!(body("---\nunclosed\n"), "---\nunclosed\n");
    assert_eq!(body(HAND), "# Body\n\n---\nnot a fence\n");
}

#[test]
fn the_metadata_is_read_from_the_frontmatter() {
    let meta = read(HAND).unwrap();
    assert_eq!(
        meta,
        Meta {
            id: Some("quiet-amber-falcon".into()),
            title: Some("Original".into()),
            status: Some("draft".into()),
            tags: vec!["area:cli".into(), "priority=3".into()],
            governs: vec![],
            supersedes: vec![],
            sessions: vec!["older".into()],
        }
    );
    assert_eq!(read("# nothing\n"), Ok(Meta::default()));
    assert_eq!(read("---\n# only a comment\n---\n"), Ok(Meta::default()));
    let lone = read("---\ngoverns: swift-amber-falcon\n---\n").unwrap();
    assert_eq!(
        lone.governs,
        ["swift-amber-falcon"],
        "a lone scalar is one item"
    );
}

#[test]
fn what_cannot_be_read_is_said() {
    assert_eq!(read("---\nid: a\n"), Err(FrontError::Unclosed));
    assert!(matches!(
        read("---\ntitle: [unclosed\n---\n"),
        Err(FrontError::Yaml { .. })
    ));
    assert_eq!(read("---\n- a\n- b\n---\n"), Err(FrontError::NotAMapping));
    assert_eq!(
        read("---\ntitle: {a: 1}\n---\n"),
        Err(FrontError::Shape {
            key: "title".into()
        })
    );
    assert_eq!(
        read("---\ntags:\n  - a: 1\n---\n"),
        Err(FrontError::Shape { key: "tags".into() })
    );
    assert_eq!(
        read("---\ntags: {a: 1}\n---\n"),
        Err(FrontError::Shape { key: "tags".into() })
    );
}

#[test]
fn a_scalar_is_set_in_place_and_every_other_byte_stays() {
    let after = edited(HAND, None, &[Edit::Set("status", "active".into())]).unwrap();
    assert_eq!(after, HAND.replacen("status: draft", "status: active", 1));
    let added = edited(HAND, None, &[Edit::Set("zeta", "z".into())]).unwrap();
    assert!(added.contains("\nzeta: z\n---\n# Body"), "{added}");
    assert_eq!(body(&added), body(HAND));
}

#[test]
fn a_list_item_is_pushed_and_removed_in_a_flow_list_and_in_a_block_one() {
    let pushed = edited(HAND, None, &[Edit::Push("tags", "human:decision".into())]).unwrap();
    assert_eq!(
        pushed,
        HAND.replacen(
            "tags: [area:cli, priority=3]",
            "tags: [area:cli, priority=3, human:decision]",
            1
        )
    );
    let removed = edited(&pushed, None, &[Edit::Remove("tags", "area:cli".into())]).unwrap();
    assert_eq!(
        removed,
        HAND.replacen(
            "tags: [area:cli, priority=3]",
            "tags: [priority=3, human:decision]",
            1
        )
    );
    let block = edited(HAND, None, &[Edit::Push("sessions", "newer".into())]).unwrap();
    assert_eq!(
        block,
        HAND.replacen("  - older\n", "  - older\n  - newer\n", 1)
    );
    let again = edited(&block, None, &[Edit::Remove("sessions", "newer".into())]).unwrap();
    assert_eq!(again, HAND);
}

#[test]
fn removing_what_is_not_there_changes_nothing_and_a_new_list_is_a_block() {
    assert_eq!(
        edited(HAND, None, &[Edit::Remove("tags", "nope".into())]).unwrap(),
        HAND
    );
    assert_eq!(
        edited(HAND, None, &[Edit::Remove("governs", "nope".into())]).unwrap(),
        HAND
    );
    let made = edited(
        HAND,
        None,
        &[Edit::Push("governs", "swift-amber-falcon".into())],
    )
    .unwrap();
    assert!(
        made.contains("\ngoverns:\n  - swift-amber-falcon\n---\n"),
        "{made}"
    );
}

#[test]
fn a_tag_that_yaml_would_read_otherwise_is_written_quoted() {
    let text = edited(
        "---\nid: a\n---\n",
        None,
        &[
            Edit::Push("tags", "area:cli".into()),
            Edit::Push("tags", "\"a/b\"".into()),
            Edit::Push("tags", "key=\"a: b\"".into()),
            Edit::Push("tags", "\"#x\"".into()),
        ],
    )
    .unwrap();
    assert!(text.contains("\n  - area:cli\n"), "{text}");
    assert!(!text.contains("\n  - \"a/b\"\n"), "{text}");
    assert_eq!(
        read(&text).unwrap().tags,
        ["area:cli", "\"a/b\"", "key=\"a: b\"", "\"#x\""]
    );
}

#[test]
fn a_file_with_no_frontmatter_gets_a_fence_block_and_keeps_every_byte_after_it() {
    let body = "# Plan\n\n---\nnot a fence\n";
    let text = edited(
        body,
        Some("quiet-amber-falcon"),
        &[Edit::Set("title", "Bare".into())],
    )
    .unwrap();
    assert_eq!(
        text,
        format!("---\nid: quiet-amber-falcon\ntitle: Bare\n---\n{body}")
    );
}

#[test]
fn an_empty_or_comment_only_frontmatter_gets_the_id_first_and_keeps_its_comments() {
    let text = edited("---\n# note\n---\nbody\n", Some("a-b"), &[]).unwrap();
    assert_eq!(text, "---\n# note\nid: a-b\n---\nbody\n");
    let text = edited("---\n---\nbody\n", Some("a-b"), &[]).unwrap();
    assert_eq!(text, "---\nid: a-b\n---\nbody\n");
}

#[test]
fn an_id_is_added_at_the_end_of_a_frontmatter_that_has_none() {
    let text = edited("---\ntitle: T\n---\nbody\n", Some("a-b"), &[]).unwrap();
    assert_eq!(text, "---\ntitle: T\nid: a-b\n---\nbody\n");
    let kept = edited(HAND, None, &[]).unwrap();
    assert_eq!(kept, HAND, "no id asked for, no change");
}

#[test]
fn an_edit_that_cannot_be_made_leaves_the_text_to_the_caller() {
    assert_eq!(edited("---\nid: a\n", None, &[]), Err(FrontError::Unclosed));
    assert!(matches!(
        edited("---\ntitle: [x\n---\n", None, &[]),
        Err(FrontError::Yaml { .. })
    ));
    assert_eq!(
        edited("---\n- a\n---\n", None, &[]),
        Err(FrontError::NotAMapping)
    );
    assert_eq!(
        edited(
            "---\ntags: one\n---\n",
            None,
            &[Edit::Push("tags", "x".into())]
        ),
        Err(FrontError::Shape { key: "tags".into() })
    );
}

#[test]
fn a_fresh_file_is_the_entries_then_the_body() {
    let text = fresh(
        vec![
            ("id", Value::scalar("quiet-amber-falcon")),
            ("title", Value::scalar("A: title")),
            (
                "governs",
                Value::Seq(vec![Value::scalar("swift-amber-falcon")]),
            ),
        ],
        "\n# A\n",
    );
    assert_eq!(
        text,
        "---\nid: quiet-amber-falcon\ntitle: \"A: title\"\ngoverns:\n  - swift-amber-falcon\n---\n\n# A\n"
    );
    let meta = read(&text).unwrap();
    assert_eq!(meta.title.as_deref(), Some("A: title"));
    assert_eq!(meta.governs, ["swift-amber-falcon"]);
}

#[test]
fn every_frontmatter_error_is_a_diagnostic_with_its_args() {
    use angzarr_diagnostic::Diagnostic;
    let errors = [
        FrontError::Unclosed,
        FrontError::Yaml { detail: "x".into() },
        FrontError::NotAMapping,
        FrontError::Shape { key: "k".into() },
        FrontError::Edit {
            key: "k".into(),
            detail: "x".into(),
        },
    ];
    let codes: Vec<&str> = errors.iter().map(Diagnostic::code).collect();
    assert_eq!(codes.len(), 5);
    assert!(codes.windows(2).all(|w| w[0] != w[1]));
    assert_eq!(
        errors[1].to_string(),
        "plans-application-front-yaml ext_err_txt=x"
    );
}
