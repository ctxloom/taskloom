//! The layers, over a home and a work root on a filesystem of the test's
//! own (`Rooted::temp`): never a real one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

crate::english_for!(&[crate::EN_US.1]);

use angzarr_embedded_diagnostic::Diagnostic;
use file_system::{Fs, Perms, Rooted};
use os::Mode;
use ports::Configuration;

use super::{ConfigError, TaskloomConfig};

/// The fixture path `path` ([`os::fixture`]) as an error names it.
fn shown(path: &str) -> String {
    os::fixture(path).display().to_string()
}

/// A home (`/home/u`) and a work root (`/tree`), each with a config file
/// holding `home` and `root` when given.
struct Dirs {
    fs: Arc<dyn Fs>,
    home: PathBuf,
    root: PathBuf,
}

impl Dirs {
    fn new(home: Option<&str>, root: Option<&str>) -> Dirs {
        let dirs = Dirs {
            fs: Arc::new(Rooted::temp().unwrap()),
            home: os::fixture("/home/u"),
            root: os::fixture("/tree"),
        };
        if let Some(text) = home {
            dirs.write(&dirs.home, text);
        }
        if let Some(text) = root {
            dirs.write(&dirs.root, text);
        }
        dirs
    }

    fn config(&self, env: &[(&str, &str)]) -> TaskloomConfig {
        TaskloomConfig::new(
            Arc::clone(&self.fs),
            Some(self.home.clone()),
            Some(self.root.clone()),
            env.iter().map(|(k, v)| (k.into(), v.into())).collect(),
            Vec::new(),
        )
    }

    fn write(&self, dir: &Path, text: &str) {
        self.fs
            .create_dir_all(&dir.join(".taskloom"), Mode::DIR)
            .unwrap();
        self.fs
            .write_atomic(
                &dir.join(".taskloom/config.yaml"),
                text.as_bytes(),
                Perms::Exact(Mode::FILE),
            )
            .unwrap();
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

/// What `config`, made with `overrides`, resolves.
fn schema(config: &TaskloomConfig, overrides: &[&str]) -> Result<Vec<String>, ConfigError> {
    let config = TaskloomConfig {
        overrides: strings(overrides),
        ..config.clone()
    };
    config.resolve()
}

const HOME: &str = "tag_schema:\n  - 'tagma.arity:\"home:x\"=scalar'\n";
const ROOT: &str = "schema_version: 1\nhoming: home\ntag_schema:\n  - 'tagma.arity:\"root:x\"=scalar'\n  - 'tagma.enum:\"root:y\"=\"a,b\"'\n";

#[test]
fn test_nothing_configured_is_no_declaration() {
    let dirs = Dirs::new(None, None);
    assert_eq!(schema(&dirs.config(&[]), &[]), Ok(Vec::new()));
    let nowhere = TaskloomConfig::new(Arc::clone(&dirs.fs), None, None, Vec::new(), Vec::new());
    assert_eq!(schema(&nowhere, &[]), Ok(Vec::new()));
}

#[test]
fn test_the_home_config_is_read() {
    let dirs = Dirs::new(Some(HOME), None);
    assert_eq!(
        schema(&dirs.config(&[]), &[]),
        Ok(strings(&[r#"tagma.arity:"home:x"=scalar"#]))
    );
}

// Each layer that sets tag_schema replaces the list below it whole.
#[test]
fn test_the_work_roots_config_replaces_the_homes_list() {
    let dirs = Dirs::new(Some(HOME), Some(ROOT));
    assert_eq!(
        schema(&dirs.config(&[]), &[]),
        Ok(strings(&[
            r#"tagma.arity:"root:x"=scalar"#,
            r#"tagma.enum:"root:y"="a,b""#
        ]))
    );
    // A root config that does not set it leaves the home's.
    let dirs = Dirs::new(Some(HOME), Some("homing: home\n"));
    assert_eq!(schema(&dirs.config(&[]), &[]).unwrap().len(), 1);
}

#[test]
fn test_the_environment_beats_the_files_and_an_override_beats_it() {
    let dirs = Dirs::new(Some(HOME), Some(ROOT));
    let env = [
        (
            "TASKLOOM_CONFIG_TAG_SCHEMA",
            r#"tagma.arity:"env:x"=scalar, tagma.enum:"env:y"="a,b""#,
        ),
        ("HOME", "/elsewhere"),
        ("CTXLOOM_CONFIG_WHATEVER", "x"),
    ];
    let config = dirs.config(&env);
    assert_eq!(
        schema(&config, &[]),
        Ok(strings(&[
            r#"tagma.arity:"env:x"=scalar"#,
            r#"tagma.enum:"env:y"="a,b""#
        ]))
    );
    assert_eq!(
        schema(
            &config,
            &[r#"Tag_Schema=tagma.arity:"set:x"=scalar"#, "homing=repo"]
        ),
        Ok(strings(&[r#"tagma.arity:"set:x"=scalar"#]))
    );
    assert_eq!(
        schema(&config, &["tag_schema=a", "tag_schema=b"]),
        Ok(strings(&["b"])),
        "the last override wins"
    );
    assert_eq!(
        schema(&config, &["tag_schema="]),
        Ok(Vec::new()),
        "set to none"
    );
}

#[test]
fn test_through_the_port() {
    let dirs = Dirs::new(None, Some(ROOT));
    let mut config = dirs.config(&[]);
    assert_eq!(config.tag_schema().unwrap().len(), 2);
    let home = Some(dirs.home.clone());
    let root = Some(dirs.root.clone());
    let set = strings(&[r#"tag_schema=tagma.arity:"set:x"=scalar"#]);
    let mut config = TaskloomConfig::new(
        Arc::clone(&dirs.fs),
        home.clone(),
        root.clone(),
        Vec::new(),
        set,
    );
    assert_eq!(
        config.tag_schema(),
        Ok(strings(&[r#"tagma.arity:"set:x"=scalar"#])),
        "the overrides it was made with, applied last"
    );
    let nonsense = strings(&["nonsense"]);
    let error = TaskloomConfig::new(Arc::clone(&dirs.fs), home, root, Vec::new(), nonsense)
        .tag_schema()
        .unwrap_err();
    assert_eq!(error.code(), "tasks-config-not-an-override");
}

#[test]
fn test_a_file_that_is_not_taskloom_config_is_refused() {
    for (text, code) in [
        ("homeing: home\n", "tasks-config-malformed"),
        ("tag_schema: not-a-list\n", "tasks-config-malformed"),
        ("[unclosed\n", "tasks-config-malformed"),
        ("schema_version: 2\n", "tasks-config-newer"),
    ] {
        let dirs = Dirs::new(None, Some(text));
        let error = schema(&dirs.config(&[]), &[]).unwrap_err();
        assert_eq!(error.code(), code, "{text}");
        assert!(error.to_string().contains("config.yaml"), "{error}");
    }
    for fine in [
        "",
        "# nothing here\n",
        "schema_version: 0\n",
        "schema_version: 1\n",
    ] {
        let dirs = Dirs::new(Some(fine), None);
        assert_eq!(schema(&dirs.config(&[]), &[]), Ok(Vec::new()), "{fine:?}");
    }
}

#[test]
fn test_a_config_file_that_cannot_be_read_is_refused() {
    let dirs = Dirs::new(None, None);
    // A directory where the file should be.
    dirs.fs
        .create_dir_all(&dirs.root.join(".taskloom/config.yaml"), Mode::DIR)
        .unwrap();
    let error = schema(&dirs.config(&[]), &[]).unwrap_err();
    assert_eq!(error.code(), "tasks-config-unreadable");
    assert!(
        error
            .to_string()
            .contains(&shown("/tree/.taskloom/config.yaml")),
        "named as the caller names it: {error}"
    );
}

#[test]
fn test_an_override_or_variable_naming_no_key_is_refused() {
    let dirs = Dirs::new(None, None);
    let error = schema(&dirs.config(&[("TASKLOOM_CONFIG_TAGS", "x")]), &[]).unwrap_err();
    assert_eq!(
        error,
        ConfigError::UnknownKey {
            source: "TASKLOOM_CONFIG_TAGS".to_string(),
            key: "tags".to_string()
        }
    );
    for bad in ["tagz=x", "=x", " =x", "tag_schema"] {
        assert!(schema(&dirs.config(&[]), &[bad]).is_err(), "{bad}");
    }
    let known = schema(
        &dirs.config(&[
            ("TASKLOOM_CONFIG_HOMING", "home"),
            ("TASKLOOM_CONFIG_SCHEMA_VERSION", "1"),
        ]),
        &["schema_version=1"],
    );
    assert_eq!(known, Ok(Vec::new()));
}

/// What `split` makes of `value`, given by `--config-set`.
fn split(value: &str) -> Result<Vec<String>, ConfigError> {
    super::split("--config-set", value)
}

#[test]
fn test_a_list_splits_on_commas_outside_quotes() {
    assert_eq!(split("a, b,c "), Ok(strings(&["a", "b", "c"])));
    assert_eq!(
        split(r#"k="a,b",x:"y,z"=1"#),
        Ok(strings(&[r#"k="a,b""#, r#"x:"y,z"=1"#]))
    );
    assert_eq!(
        split(r#"k="a\",b",c"#),
        Ok(strings(&[r#"k="a\",b""#, "c"])),
        "an escaped quote does not close"
    );
    assert_eq!(split(""), Ok(Vec::new()));
    assert_eq!(split("  "), Ok(Vec::new()), "no item, so none blank");
    assert_eq!(
        split(r#"k="a,b"#),
        Err(ConfigError::Unterminated {
            source: "--config-set".to_string(),
            value: r#"k="a,b"#.to_string(),
        })
    );
    let dirs = Dirs::new(None, None);
    let error = schema(&dirs.config(&[]), &[r#"tag_schema=k="a"#]).unwrap_err();
    assert_eq!(error.code(), "tasks-config-unterminated");
}

// A variable's name is compared as the host compares names: exactly on
// unix, in any case on Windows (os's Flavor::env_has_prefix).
#[test]
fn test_a_variable_is_named_as_the_host_names_it() {
    let dirs = Dirs::new(Some(HOME), Some(ROOT));
    let lower = dirs.config(&[(
        "taskloom_config_tag_schema",
        r#"tagma.arity:"env:x"=scalar"#,
    )]);
    let read = schema(&lower, &[]) == Ok(strings(&[r#"tagma.arity:"env:x"=scalar"#]));
    assert_eq!(read, os::host().flavor() == os::Flavor::Windows);
}

// An empty variable of the environment is unset, as everywhere else
// (CTXLOOM_PROJECT_ID, CTXLOOM_SESSION_HARP): `TASKLOOM_CONFIG_TAG_SCHEMA=`
// left over in a shell is no layer, never an empty list that drops every
// declaration the files make (their enums, ranges and scalar targets).
#[test]
fn test_an_empty_variable_is_no_layer() {
    let dirs = Dirs::new(Some(HOME), Some(ROOT));
    let config = dirs.config(&[("TASKLOOM_CONFIG_TAG_SCHEMA", "")]);
    assert_eq!(schema(&config, &[]), schema(&dirs.config(&[]), &[]));
    let config = dirs.config(&[("TASKLOOM_CONFIG_TAGS", "")]);
    assert_eq!(
        schema(&config, &[]).map(|s| s.len()),
        Ok(2),
        "unset, so naming no key is no refusal"
    );
    // An override is given, so its empty list is one.
    assert_eq!(schema(&dirs.config(&[]), &["tag_schema="]), Ok(Vec::new()));
}

// A blank item is refused, never dropped: `a,,b`, a trailing comma or
// `"$A,$B"` with B unset is a list given wrong, and dropping the item would
// quietly lose the declaration the caller meant to give.
#[test]
fn test_a_blank_item_in_a_list_is_refused() {
    for value in ["a,,b", "a,", ",a", "a, ,b", ",", " , "] {
        assert_eq!(
            split(value),
            Err(ConfigError::BlankItem {
                source: "--config-set".to_string(),
                value: value.to_string(),
            }),
            "{value:?}"
        );
    }
    assert_eq!(
        split(r#"k=",",a"#),
        Ok(strings(&[r#"k=",""#, "a"])),
        "a quoted comma"
    );
    let dirs = Dirs::new(Some(HOME), None);
    let error = schema(&dirs.config(&[]), &["tag_schema=a,"]).unwrap_err();
    assert_eq!(error.code(), "tasks-config-blank-item");
    let config = dirs.config(&[("TASKLOOM_CONFIG_TAG_SCHEMA", "a,")]);
    assert_eq!(
        schema(&config, &[]),
        Err(ConfigError::BlankItem {
            source: "TASKLOOM_CONFIG_TAG_SCHEMA".to_string(),
            value: "a,".to_string(),
        })
    );
}

// A config file's list is held to the same rule: a blank item in it is
// refused, the file and its key named, never dropped.
#[test]
fn test_a_blank_item_in_a_config_files_list_is_refused() {
    for item in ["''", "'  '"] {
        let text = format!("tag_schema:\n  - 'a'\n  - {item}\n");
        let dirs = Dirs::new(None, Some(&text));
        assert_eq!(
            schema(&dirs.config(&[]), &[]),
            Err(ConfigError::BlankItem {
                source: shown("/tree/.taskloom/config.yaml"),
                value: "tag_schema".to_string(),
            }),
            "{item}"
        );
    }
    let dirs = Dirs::new(Some("tag_schema:\n  - ''\n"), None);
    let error = schema(&dirs.config(&[]), &[]).unwrap_err();
    assert_eq!(error.code(), "tasks-config-blank-item");
    assert_eq!(
        error.english(),
        format!(
            "{}: tag_schema has a blank item: give one, or leave it out",
            shown("/home/u/.taskloom/config.yaml")
        )
    );
}

// The environment is given as the process has it; a variable whose name or
// value is not UTF-8 is skipped where the configuration reads it, never
// refused, and the rest still read.
#[test]
fn test_a_variable_that_is_not_utf8_is_skipped() {
    let dirs = Dirs::new(Some(HOME), None);
    let set: (OsString, OsString) = (
        "TASKLOOM_CONFIG_TAG_SCHEMA".into(),
        r#"tagma.arity:"env:x"=scalar"#.into(),
    );
    for odd in [
        (os::not_unicode("TASKLOOM_CONFIG_"), OsString::from("x")),
        (
            OsString::from("TASKLOOM_CONFIG_TAG_SCHEMA"),
            os::not_unicode(""),
        ),
    ] {
        let config = TaskloomConfig::new(
            Arc::clone(&dirs.fs),
            Some(dirs.home.clone()),
            None,
            vec![set.clone(), odd],
            Vec::new(),
        );
        assert_eq!(
            schema(&config, &[]),
            Ok(strings(&[r#"tagma.arity:"env:x"=scalar"#]))
        );
    }
}

// The work root may be found only when the configuration is read: making
// it walks nothing, so a command that reads no configuration does no I/O.
#[test]
fn test_a_work_root_found_on_reading_is_not_sought_until_then() {
    let dirs = Dirs::new(None, Some(ROOT));
    let sought = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&sought);
    let root = dirs.root.clone();
    let mut config = TaskloomConfig::finding_root(
        Arc::clone(&dirs.fs),
        None,
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Some(root.clone())
        },
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(sought.load(Ordering::SeqCst), 0, "made: nothing sought");
    assert!(format!("{config:?}").contains("FindRoot"), "{config:?}");
    assert_eq!(config.tag_schema().unwrap().len(), 2, "the root's list");
    assert_eq!(sought.load(Ordering::SeqCst), 1);
}

// ------------------------------------------------------------ tag permits
//
// The shape, in the work root's .taskloom/config.yaml alone:
//
//     tag_permits:
//       namespaces:
//         area:          # closed: only these keys
//           - cli
//           - mcp
//         relates: open  # open: any key
//       tags:            # flat tags (no namespace)
//         - urgent

mod permits {
    use std::collections::{BTreeMap, BTreeSet};

    use ports::{ConfigurationWriter, NamespacePermit, TagPermits};

    use super::*;

    const PERMITS: &str = "tag_permits:\n  namespaces:\n    area:\n      - cli\n      - mcp\n    relates: open\n  tags:\n    - urgent\n";

    fn usual() -> TagPermits {
        let keys = |k: &[&str]| k.iter().map(|k| k.to_string()).collect::<BTreeSet<_>>();
        TagPermits {
            namespaces: BTreeMap::from([
                (
                    "area".to_string(),
                    NamespacePermit::Closed(keys(&["cli", "mcp"])),
                ),
                ("relates".to_string(), NamespacePermit::Open),
            ]),
            flat: keys(&["urgent"]),
        }
    }

    fn file(dirs: &Dirs) -> String {
        dirs.fs
            .read_to_string(&dirs.root.join(".taskloom/config.yaml"))
            .unwrap()
    }

    #[test]
    fn test_the_work_roots_tag_permits_are_read() {
        let dirs = Dirs::new(None, Some(PERMITS));
        assert_eq!(dirs.config(&[]).tag_permits(), Ok(Some(usual())));
    }

    // Lists in flow style read as in block style: the proposed vocabulary
    // (tag-vocabulary.proposal.md, section 5) is written so.
    #[test]
    fn test_tag_permits_in_flow_style_are_read() {
        let flow = "tag_permits:\n  namespaces:\n    area: [cli, mcp]\n    relates: open\n  tags: [urgent]\n";
        let dirs = Dirs::new(None, Some(flow));
        assert_eq!(dirs.config(&[]).tag_permits(), Ok(Some(usual())));
    }

    // PROPOSAL (open question 2, opt-in): no tag_permits is None, which
    // the use cases read as permissive; an empty section is permits that
    // permit nothing.
    #[test]
    fn test_no_tag_permits_is_none_and_an_empty_section_is_none_permitted() {
        for text in [None, Some(ROOT)] {
            let dirs = Dirs::new(None, text);
            assert_eq!(dirs.config(&[]).tag_permits(), Ok(None), "{text:?}");
        }
        let dirs = Dirs::new(None, Some("tag_permits: {}\n"));
        assert_eq!(
            dirs.config(&[]).tag_permits(),
            Ok(Some(TagPermits::default()))
        );
    }

    // PROPOSAL: a project's vocabulary is its own (ruling 4): permits in
    // the home's config are refused, never merged or silently ignored.
    #[test]
    fn test_tag_permits_in_the_homes_config_are_refused() {
        let dirs = Dirs::new(Some(PERMITS), None);
        let error = dirs.config(&[]).tag_permits().unwrap_err();
        assert_eq!(error.code(), "tasks-config-permits-outside-project");
        assert!(error
            .to_string()
            .contains(&shown("/home/u/.taskloom/config.yaml")));
    }

    // PROPOSAL: permits are not layered: no variable or --config-set sets
    // them, so a run cannot widen the vocabulary for itself.
    #[test]
    fn test_tag_permits_cannot_be_set_by_a_variable_or_override() {
        let dirs = Dirs::new(None, None);
        let error = schema(&dirs.config(&[]), &["tag_permits=x"]).unwrap_err();
        assert_eq!(
            error,
            ConfigError::FileOnly {
                source: "--config-set".to_string(),
                key: "tag_permits".to_string()
            }
        );
        let env = [("TASKLOOM_CONFIG_TAG_PERMITS", "x")];
        let error = schema(&dirs.config(&env), &[]).unwrap_err();
        assert_eq!(error.code(), "tasks-config-file-only");
    }

    #[test]
    fn test_tag_permits_not_of_their_shape_are_refused() {
        for (text, code) in [
            (
                "tag_permits:\n  namespaces:\n    area: maybe\n",
                "tasks-config-malformed",
            ),
            ("tag_permits:\n  areas: {}\n", "tasks-config-malformed"),
            ("tag_permits:\n  tags: urgent\n", "tasks-config-malformed"),
            (
                "tag_permits:\n  namespaces:\n    area: ['cli', '']\n",
                "tasks-config-blank-item",
            ),
            ("tag_permits:\n  tags: ['']\n", "tasks-config-blank-item"),
        ] {
            let dirs = Dirs::new(None, Some(text));
            let error = dirs.config(&[]).tag_permits().unwrap_err();
            assert_eq!(error.code(), code, "{text}");
        }
    }

    // A config file a person wrote: comments above, beside and inside
    // the tag_permits block, a blank line, and keys on either side.
    const COMMENTED: &str = "# Taskloom, for this project.\nschema_version: 1   # pinned\n\n# What tags may be used:\ntag_permits:\n  namespaces:\n    # Where in the code.\n    area:\n      - cli    # the command line\n      - mcp\n      - store  # state.db\n    queue: open  # nightly runs\n  # Flat tags, kept few.\n  tags:\n    - urgent  # rare\n    - wibble\n\n# The schema.\ntag_schema:\n  - 'tagma.arity:\"area:x\"=scalar'\n";

    fn closed(keys: &[&str]) -> NamespacePermit {
        NamespacePermit::Closed(keys.iter().map(|k| k.to_string()).collect())
    }

    fn flat(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    /// COMMENTED's permits, as read.
    fn commented() -> TagPermits {
        TagPermits {
            namespaces: BTreeMap::from([
                ("area".to_string(), closed(&["cli", "mcp", "store"])),
                ("queue".to_string(), NamespacePermit::Open),
            ]),
            flat: flat(&["urgent", "wibble"]),
        }
    }

    // Ruling 11 (yaml-rt-core): a permit is the fewest edits to the block,
    // so every comment survives, INSIDE the block too. A key joins the end
    // of its namespace's list, a namespace the end of `namespaces`, a flat
    // tag the end of `tags`: the person's order is kept, nothing sorted.
    #[test]
    fn test_a_permit_added_keeps_every_comment_in_the_file() {
        let dirs = Dirs::new(None, Some(COMMENTED));
        let mut permits = commented();
        permits.namespaces.insert(
            "area".to_string(),
            closed(&["cli", "config", "mcp", "store"]),
        );
        permits
            .namespaces
            .insert("sig".to_string(), NamespacePermit::Open);
        permits.flat.insert("blocked".to_string());
        dirs.config(&[]).write_tag_permits(&permits).unwrap();
        let expected = COMMENTED
            .replace(
                "      - store  # state.db\n",
                "      - store  # state.db\n      - config\n",
            )
            .replace(
                "    queue: open  # nightly runs\n",
                "    queue: open  # nightly runs\n    sig: open\n",
            )
            .replace("    - wibble\n", "    - wibble\n    - blocked\n");
        assert_eq!(file(&dirs), expected);
        assert_eq!(dirs.config(&[]).tag_permits(), Ok(Some(permits)));
    }

    // A revoke removes the entry and the comment on its own line, and
    // nothing else: the comments above and beside the entries left stay.
    #[test]
    fn test_a_permit_removed_keeps_the_comments_around_it() {
        let dirs = Dirs::new(None, Some(COMMENTED));
        let mut permits = commented();
        permits
            .namespaces
            .insert("area".to_string(), closed(&["cli", "store"]));
        permits.namespaces.remove("queue");
        permits.flat.remove("urgent");
        dirs.config(&[]).write_tag_permits(&permits).unwrap();
        let expected = COMMENTED
            .replace("      - mcp\n", "")
            .replace("    queue: open  # nightly runs\n", "")
            .replace("    - urgent  # rare\n", "");
        assert_eq!(file(&dirs), expected);
        assert_eq!(dirs.config(&[]).tag_permits(), Ok(Some(permits)));
    }

    // A namespace turning open or closed is rewritten as one entry
    // (closing an open one is a removal and an addition: yaml-rt-core
    // cannot turn a scalar into a block list in place). Its own comments
    // may go with it; every other line of the file stays.
    #[test]
    fn test_opening_or_closing_a_namespace_keeps_the_rest_of_the_file() {
        let dirs = Dirs::new(None, Some(COMMENTED));
        let mut permits = commented();
        permits
            .namespaces
            .insert("area".to_string(), NamespacePermit::Open);
        permits
            .namespaces
            .insert("queue".to_string(), closed(&["nightly"]));
        dirs.config(&[]).write_tag_permits(&permits).unwrap();
        assert_eq!(dirs.config(&[]).tag_permits(), Ok(Some(permits)));
        let written = file(&dirs);
        for kept in [
            "# Taskloom, for this project.\nschema_version: 1   # pinned\n\n# What tags may be used:\ntag_permits:\n  namespaces:\n",
            "  # Flat tags, kept few.\n  tags:\n    - urgent  # rare\n    - wibble\n\n# The schema.\ntag_schema:\n  - 'tagma.arity:\"area:x\"=scalar'\n",
        ] {
            assert!(written.contains(kept), "{written}");
        }
    }

    // A list written in flow style (`[a, b]`) is edited in its own style,
    // its comment kept.
    #[test]
    fn test_a_flow_list_is_edited_in_its_own_style() {
        let dirs = Dirs::new(
            None,
            Some("tag_permits:\n  namespaces:\n    area: [cli, mcp, store]  # ours\n"),
        );
        let permits = TagPermits {
            namespaces: BTreeMap::from([("area".to_string(), closed(&["cli", "config", "store"]))]),
            flat: BTreeSet::new(),
        };
        dirs.config(&[]).write_tag_permits(&permits).unwrap();
        assert_eq!(
            file(&dirs),
            "tag_permits:\n  namespaces:\n    area: [cli, store, config]  # ours\n"
        );
    }

    // Permits already as meant are not written: the file is left byte
    // for byte.
    #[test]
    fn test_writing_the_permits_already_there_leaves_the_file_alone() {
        let dirs = Dirs::new(None, Some(COMMENTED));
        dirs.config(&[]).write_tag_permits(&commented()).unwrap();
        assert_eq!(file(&dirs), COMMENTED);
        assert_eq!(dirs.config(&[]).tag_permits(), Ok(Some(commented())));
    }

    #[test]
    fn test_writing_tag_permits_appends_them_or_makes_the_file() {
        let dirs = Dirs::new(None, Some("# mine\nhoming: home\n"));
        dirs.config(&[]).write_tag_permits(&usual()).unwrap();
        assert_eq!(file(&dirs), format!("# mine\nhoming: home\n{PERMITS}"));
        let dirs = Dirs::new(None, None);
        dirs.config(&[]).write_tag_permits(&usual()).unwrap();
        assert_eq!(file(&dirs), PERMITS);
        let dirs = Dirs::new(None, None);
        dirs.config(&[])
            .write_tag_permits(&TagPermits::default())
            .unwrap();
        assert_eq!(file(&dirs), "tag_permits: {}\n");
    }

    #[test]
    fn test_writing_tag_permits_with_no_work_root_is_refused() {
        let dirs = Dirs::new(None, None);
        let mut rootless = TaskloomConfig::new(
            Arc::clone(&dirs.fs),
            Some(dirs.home.clone()),
            None,
            Vec::new(),
            Vec::new(),
        );
        let error = rootless.write_tag_permits(&usual()).unwrap_err();
        assert_eq!(error.code(), "tasks-config-no-work-root");
        assert!(
            dirs.fs
                .read_to_string(&dirs.home.join(".taskloom/config.yaml"))
                .is_err(),
            "the home's config is never written"
        );
    }
}
