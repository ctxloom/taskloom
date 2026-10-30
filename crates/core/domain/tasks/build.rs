//! The tasks domain layer (domain types, wire conversions, payload enums and
//! their `DomainMessage` impls: angzarr-build's markup layer), generated
//! into `OUT_DIR` from the protos. Generated code is never committed.

fn main() -> Result<(), angzarr_build::BuildError> {
    angzarr_build::markup_domain(
        &["ctxloom/tasks/v1/tasks.proto"],
        &["../../../../proto", tagma_proto::PROTO_INCLUDE],
        &[("ctxloom.tasks.v1", "::tasks_proto::v1")],
    )
}
