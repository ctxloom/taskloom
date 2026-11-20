//! What differs between two snapshots of the plan files.

use super::*;

fn watched(id: Option<&str>, meta: Meta, body: &str) -> Watched {
    Watched {
        id: id.map(str::to_string),
        meta: Meta {
            id: id.map(str::to_string),
            ..meta
        },
        body: body.to_string(),
        invalid: None,
    }
}

fn snapshot(files: &[(&str, Watched)]) -> Snapshot {
    files
        .iter()
        .map(|(path, w)| (path.to_string(), w.clone()))
        .collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|i| i.to_string()).collect()
}

fn one() -> Watched {
    watched(
        Some("quiet-amber-falcon"),
        Meta {
            title: Some("T".into()),
            status: Some("draft".into()),
            tags: strings(&["a", "b"]),
            ..Meta::default()
        },
        "# T\n",
    )
}

#[test]
fn identical_snapshots_say_nothing() {
    let both = snapshot(&[("/p.plan.md", one())]);
    assert_eq!(changes(&both, &both.clone()), []);
    assert_eq!(changes(&Snapshot::new(), &Snapshot::new()), []);
}

#[test]
fn a_created_file_lists_every_key_it_has_as_an_addition() {
    let new = snapshot(&[("/p.plan.md", one())]);
    let found = changes(&Snapshot::new(), &new);
    assert_eq!(found.len(), 1);
    let change = &found[0];
    assert_eq!(change.event, Event::Created);
    assert_eq!(change.id.as_deref(), Some("quiet-amber-falcon"));
    assert_eq!(change.path, "/p.plan.md");
    assert_eq!(
        change.deltas,
        [
            Delta::Scalar {
                key: "id",
                from: None,
                to: Some("quiet-amber-falcon".into())
            },
            Delta::Scalar {
                key: "title",
                from: None,
                to: Some("T".into())
            },
            Delta::Scalar {
                key: "status",
                from: None,
                to: Some("draft".into())
            },
            Delta::List {
                key: "tags",
                added: strings(&["a", "b"]),
                removed: vec![]
            },
        ],
        "and no body: a created file's text is read from the file"
    );
}

#[test]
fn a_deleted_file_keeps_its_last_id_and_path_and_has_no_changes() {
    let old = snapshot(&[("/p.plan.md", one())]);
    let found = changes(&old, &Snapshot::new());
    assert_eq!(
        found,
        [Change {
            event: Event::Deleted,
            id: Some("quiet-amber-falcon".into()),
            path: "/p.plan.md".into(),
            deltas: vec![],
            invalid: None,
        }]
    );
}

#[test]
fn a_changed_file_names_each_key_that_differs() {
    let old = snapshot(&[("/p.plan.md", one())]);
    let mut moved = one();
    moved.meta.status = Some("active".into());
    moved.meta.tags = strings(&["b", "c"]);
    moved.meta.sessions = strings(&["s"]);
    moved.body = "# T\nmore\n".into();
    let found = changes(&old, &snapshot(&[("/p.plan.md", moved)]));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].event, Event::Changed);
    assert_eq!(
        found[0].deltas,
        [
            Delta::Scalar {
                key: "status",
                from: Some("draft".into()),
                to: Some("active".into())
            },
            Delta::List {
                key: "tags",
                added: strings(&["c"]),
                removed: strings(&["a"])
            },
            Delta::List {
                key: "sessions",
                added: strings(&["s"]),
                removed: vec![]
            },
            Delta::Body,
        ]
    );
}

#[test]
fn only_the_body_differing_is_only_the_body() {
    let old = snapshot(&[("/p.plan.md", one())]);
    let mut edited = one();
    edited.body = "# Other\n".into();
    let found = changes(&old, &snapshot(&[("/p.plan.md", edited)]));
    assert_eq!(found[0].deltas, [Delta::Body]);
}

#[test]
fn a_key_dropped_is_a_scalar_to_nothing_and_files_come_in_path_order() {
    let old = snapshot(&[("/b.plan.md", one()), ("/a.plan.md", one())]);
    let mut dropped = one();
    dropped.meta.title = None;
    let new = snapshot(&[("/b.plan.md", dropped), ("/c.plan.md", one())]);
    let found = changes(&old, &new);
    let order: Vec<(&str, Event)> = found.iter().map(|c| (c.path.as_str(), c.event)).collect();
    assert_eq!(
        order,
        [
            ("/a.plan.md", Event::Deleted),
            ("/b.plan.md", Event::Changed),
            ("/c.plan.md", Event::Created)
        ]
    );
    assert_eq!(
        found[1].deltas,
        [Delta::Scalar {
            key: "title",
            from: Some("T".into()),
            to: None
        }]
    );
}

#[test]
fn a_frontmatter_that_cannot_be_read_is_carried_on_the_change() {
    let mut broken = one();
    broken.invalid = Some(FrontError::Unclosed);
    let found = changes(&Snapshot::new(), &snapshot(&[("/p.plan.md", broken)]));
    assert_eq!(found[0].invalid, Some(FrontError::Unclosed));
}
