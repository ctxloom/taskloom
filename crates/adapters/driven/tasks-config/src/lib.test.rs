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

/// What `config`, made with `overrides`, says of the moved keys.
fn moved(config: &TaskloomConfig, overrides: &[&str]) -> Result<(), ConfigError> {
    let config = TaskloomConfig {
        overrides: strings(overrides),
        ..config.clone()
    };
    config.moved_keys()
}

const HOME: &str = "tag_schema:\n  - 'tagma.arity:\"home:x\"=scalar'\n";
const ROOT: &str = "schema_version: 1\nhoming: home\ntag_schema:\n  - 'tagma.arity:\"root:x\"=scalar'\n  - 'tagma.enum:\"root:y\"=\"a,b\"'\n";

// The tag rules moved into the store (docs/plans/self-hosted-schema.md):
// nothing configured sets them, and that is no refusal.
#[test]
fn test_nothing_configured_moves_nothing() {
    let dirs = Dirs::new(Some("homing: home\n"), Some("schema_version: 1\n"));
    assert_eq!(moved(&dirs.config(&[]), &[]), Ok(()));
    let nowhere = TaskloomConfig::new(Arc::clone(&dirs.fs), None, None, Vec::new(), Vec::new());
    assert_eq!(moved(&nowhere, &[]), Ok(()));
    let known = dirs.config(&[
        ("TASKLOOM_CONFIG_HOMING", "home"),
        ("TASKLOOM_CONFIG_SCHEMA_VERSION", "1"),
    ]);
    assert_eq!(moved(&known, &["schema_version=1", "Homing=repo"]), Ok(()));
}

// Was: each layer's tag_schema replaced the one below it. Now every layer
// still setting a moved key, a file's tag_schema or tag_permits, the
// variable or an override, is refused naming it and the import.
#[test]
fn test_every_layer_setting_a_moved_key_is_refused_naming_it() {
    let file = |dir: &str| shown(&format!("{dir}/.taskloom/config.yaml"));
    for (home, root, key, source) in [
        (Some(HOME), None, "tag_schema", file("/home/u")),
        (None, Some(ROOT), "tag_schema", file("/tree")),
        (
            None,
            Some("tag_permits:\n  tags: [urgent]\n"),
            "tag_permits",
            file("/tree"),
        ),
        (
            Some("tag_permits:\n  tags: [urgent]\n"),
            None,
            "tag_permits",
            file("/home/u"),
        ),
    ] {
        let dirs = Dirs::new(home, root);
        let error = moved(&dirs.config(&[]), &[]).unwrap_err();
        assert_eq!(
            error,
            ConfigError::KeyMoved {
                source: source.clone(),
                key: key.to_string()
            },
            "{source}"
        );
        assert!(
            error.english().contains("taskloom schema import-config"),
            "{error}"
        );
    }
    let dirs = Dirs::new(None, None);
    for (env, overrides, source, key) in [
        (
            &[("TASKLOOM_CONFIG_TAG_SCHEMA", "x")][..],
            &[][..],
            "TASKLOOM_CONFIG_TAG_SCHEMA",
            "tag_schema",
        ),
        (
            &[("TASKLOOM_CONFIG_TAG_PERMITS", "x")],
            &[],
            "TASKLOOM_CONFIG_TAG_PERMITS",
            "tag_permits",
        ),
        (&[], &["Tag_Schema=x"], "--config-set", "tag_schema"),
        (&[], &["tag_permits=x"], "--config-set", "tag_permits"),
        (&[], &["tag_schema="], "--config-set", "tag_schema"),
    ] {
        assert_eq!(
            moved(&dirs.config(env), overrides),
            Err(ConfigError::KeyMoved {
                source: source.to_string(),
                key: key.to_string()
            }),
            "{source}"
        );
    }
}

#[test]
fn test_through_the_port() {
    let dirs = Dirs::new(None, Some(ROOT));
    let error = dirs.config(&[]).moved().unwrap_err();
    assert_eq!(error.code(), "tasks-config-key-moved");
    let nonsense = strings(&["nonsense"]);
    let error = TaskloomConfig::new(
        Arc::clone(&dirs.fs),
        None,
        Some(dirs.root.clone()),
        Vec::new(),
        nonsense,
    )
    .moved()
    .unwrap_err();
    assert_eq!(
        error.code(),
        "tasks-config-key-moved",
        "the root's file first"
    );
    let dirs = Dirs::new(None, None);
    let error = TaskloomConfig::new(
        Arc::clone(&dirs.fs),
        None,
        None,
        Vec::new(),
        strings(&["nonsense"]),
    )
    .moved()
    .unwrap_err();
    assert_eq!(error.code(), "tasks-config-not-an-override");
}

// The import alone reads the work root's file's tag_schema, as written,
// and no other layer's.
#[test]
fn test_the_import_reads_the_work_roots_tag_schema_alone() {
    let dirs = Dirs::new(Some(HOME), Some(ROOT));
    let env = [("TASKLOOM_CONFIG_TAG_SCHEMA", "tagma.arity:\"env:x\"=scalar")];
    assert_eq!(
        dirs.config(&env).tag_schema(),
        Ok(strings(&[
            r#"tagma.arity:"root:x"=scalar"#,
            r#"tagma.enum:"root:y"="a,b""#
        ]))
    );
    let dirs = Dirs::new(Some(HOME), Some("homing: home\n"));
    assert_eq!(
        dirs.config(&[]).tag_schema(),
        Ok(Vec::new()),
        "not the home's"
    );
    let rootless = TaskloomConfig::new(
        Arc::clone(&dirs.fs),
        Some(dirs.home.clone()),
        None,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(rootless.clone().tag_schema(), Ok(Vec::new()));
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
        let error = moved(&dirs.config(&[]), &[]).unwrap_err();
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
        assert_eq!(moved(&dirs.config(&[]), &[]), Ok(()), "{fine:?}");
    }
}

#[test]
fn test_a_config_file_that_cannot_be_read_is_refused() {
    let dirs = Dirs::new(None, None);
    // A directory where the file should be.
    dirs.fs
        .create_dir_all(&dirs.root.join(".taskloom/config.yaml"), Mode::DIR)
        .unwrap();
    let error = moved(&dirs.config(&[]), &[]).unwrap_err();
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
    let error = moved(&dirs.config(&[("TASKLOOM_CONFIG_TAGS", "x")]), &[]).unwrap_err();
    assert_eq!(
        error,
        ConfigError::UnknownKey {
            source: "TASKLOOM_CONFIG_TAGS".to_string(),
            key: "tags".to_string()
        }
    );
    for bad in ["tagz=x", "=x", " =x", "tag_schema"] {
        assert!(moved(&dirs.config(&[]), &[bad]).is_err(), "{bad}");
    }
}

// A variable's name is compared as the host compares names: exactly on
// unix, in any case on Windows (os's Flavor::env_has_prefix).
#[test]
fn test_a_variable_is_named_as_the_host_names_it() {
    let dirs = Dirs::new(None, None);
    let lower = dirs.config(&[(
        "taskloom_config_tag_schema",
        r#"tagma.arity:"env:x"=scalar"#,
    )]);
    let refused = moved(&lower, &[]).is_err();
    assert_eq!(refused, os::host().flavor() == os::Flavor::Windows);
}

// An empty variable of the environment is unset, as everywhere else:
// `TASKLOOM_CONFIG_TAG_SCHEMA=` left over in a shell is no layer, so it
// sets nothing that moved. An override is given, so even empty it is one.
#[test]
fn test_an_empty_variable_is_no_layer() {
    let dirs = Dirs::new(None, None);
    let config = dirs.config(&[("TASKLOOM_CONFIG_TAG_SCHEMA", "")]);
    assert_eq!(moved(&config, &[]), Ok(()));
    let config = dirs.config(&[("TASKLOOM_CONFIG_TAGS", "")]);
    assert_eq!(
        moved(&config, &[]),
        Ok(()),
        "unset, so naming no key is no refusal"
    );
}

// A config file's list is held to taskloom's rule: a blank item in it is
// refused, the file and its key named, never dropped, when the import
// reads it.
#[test]
fn test_a_blank_item_in_a_config_files_list_is_refused() {
    for item in ["''", "'  '"] {
        let text = format!("tag_schema:\n  - 'a'\n  - {item}\n");
        let dirs = Dirs::new(None, Some(&text));
        let error = dirs.config(&[]).tag_schema().unwrap_err();
        assert_eq!(error.code(), "tasks-config-blank-item", "{item}");
    }
    let dirs = Dirs::new(Some("tag_schema:\n  - ''\n"), None);
    let error = moved(&dirs.config(&[]), &[]).unwrap_err();
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
// refused.
#[test]
fn test_a_variable_that_is_not_utf8_is_skipped() {
    let dirs = Dirs::new(None, None);
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
            vec![odd],
            Vec::new(),
        );
        assert_eq!(moved(&config, &[]), Ok(()));
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

// The import takes the moved keys out of the work root's file, its other
// keys and comments kept; with neither key there, nothing is written.
#[test]
fn test_retiring_takes_the_moved_keys_out_of_the_work_roots_file() {
    use ports::ConfigurationWriter;
    let dirs = Dirs::new(Some(HOME), Some(ROOT));
    assert_eq!(dirs.config(&[]).retire_moved(), Ok(true));
    let file = |dir: &Path| {
        dirs.fs
            .read_to_string(&dir.join(".taskloom/config.yaml"))
            .unwrap()
    };
    assert_eq!(file(&dirs.root), "schema_version: 1\nhoming: home\n");
    assert_eq!(file(&dirs.home), HOME, "the home's is never written");
    assert_eq!(dirs.config(&[]).retire_moved(), Ok(false), "once");
    let dirs = Dirs::new(None, None);
    assert_eq!(dirs.config(&[]).retire_moved(), Ok(false), "no file");
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
