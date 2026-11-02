//! The plans domain layer (domain types, wire conversions and their
//! `DomainMessage` impls: angzarr-build's markup layer), generated into
//! `OUT_DIR` from the protos. Generated code is never committed.
//! And the crate's catalogs, written to OUT_DIR under its package name.

fn main() -> Result<(), angzarr_build::BuildError> {
    angzarr_diagnostic::build::catalogs();
    angzarr_build::markup_domain(
        &["ctxloom/plans/v1/plans.proto"],
        &["../../../../proto"],
        &[("ctxloom.plans.v1", "::plans_proto::v1")],
    )
}
