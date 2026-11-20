//! The tasks domain layer (domain types, wire conversions, payload enums and
//! their `DomainMessage` impls: angzarr-build's markup layer), generated
//! into `OUT_DIR` from the protos. Generated code is never committed.
//! And the crate's catalogs, written to OUT_DIR under its package name.

fn main() -> Result<(), angzarr_build::BuildError> {
    angzarr_diagnostic::build::catalogs();
    angzarr_build::markup_domain_with(
        &[
            "ctxloom/tasks/v1/tasks.proto",
            "ctxloom/tasks/v1/schema.proto",
        ],
        &["../../../../proto", tagma_proto::PROTO_INCLUDE],
        &[
            ("ctxloom.tasks.v1", "::tasks_proto::v1"),
            ("tagma.v1", "::tasks_proto::tagma::v1"),
        ],
        &[
            ("ctxloom.tasks.v1", "crate::generated"),
            ("tagma.v1", "crate::tagma_layer"),
        ],
    )
}
