//! The layers, over a home and a work root on a filesystem of the test's
//! own (`Rooted::temp`): never a real one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use annals_diagnostic::Diagnostic;
use file_system::{Fs, Mode, Perms, Rooted};
use ports::Configuration;

use super::{ConfigError, TaskloomConfig};

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
            home: PathBuf::from("/home/u"),
            root: PathBuf::from("/tree"),
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
            env.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
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
        error.to_string().contains("/tree/.taskloom/config.yaml"),
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
