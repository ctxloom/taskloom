//! A diagnostic's en-US message, from this crate's catalogs (and the
//! domain's and task-priority's): what an edge shows, for the tests that
//! hold the English to Go's. A code no catalog has is its key and args, as
//! an edge shows it.

use angzarr_embedded_diagnostic::{Arg, Diagnostic};
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};

/// `d`'s en-US message.
pub fn english(d: &dyn Diagnostic) -> String {
    let mut bundle = FluentBundle::new(vec!["en-US".parse().expect("a locale")]);
    bundle.set_use_isolating(false);
    for (_, ftl) in crate::CATALOGS {
        let resource = FluentResource::try_new(ftl.to_string()).expect("the catalog parses");
        bundle.add_resource(resource).expect("no message twice");
    }
    render(&bundle, d)
}

fn render(bundle: &FluentBundle<FluentResource>, d: &dyn Diagnostic) -> String {
    let Some(pattern) = bundle.get_message(d.code()).and_then(|m| m.value()) else {
        return d.to_string();
    };
    let mut args = FluentArgs::new();
    for (name, arg) in d.args() {
        let value: FluentValue = match arg {
            Arg::Int(n) => (n as f64).into(),
            Arg::Uint(n) => (n as f64).into(),
            Arg::Str(s) => s.into(),
            Arg::Nested(cause) => render(bundle, &*cause).into(),
        };
        args.set(name, value);
    }
    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, Some(&args), &mut errors);
    assert!(errors.is_empty(), "{}: {errors:?}", d.code());
    text.into_owned()
}

/// [`english`], as a method of every diagnostic.
pub trait English {
    fn english(&self) -> String;
}

impl<D: Diagnostic> English for D {
    fn english(&self) -> String {
        english(self)
    }
}
