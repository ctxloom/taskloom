//! The plans domain's wire types: prost messages generated from
//! `proto/ctxloom/plans/v1/plans.proto`. They are the wire form only; domain
//! code works on domain types converted from these at the boundary.

/// The generated package, nested as its proto package is.
pub mod ctxloom {
    pub mod plans {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/ctxloom.plans.v1.rs"));
        }
    }
}

/// `ctxloom.plans.v1`.
pub use ctxloom::plans::v1;

/// The encoded `FileDescriptorSet` of the plans protos and every file they
/// import (angzarr's and tagma's among them), as protoc compiled them for
/// this crate: angzarr-build's `wire` writes it beside the generated code,
/// as `angzarr.descriptors`. What a store export expands this domain's
/// stored payloads and states through (store-export-import.plan.md,
/// D3 (a)).
pub const FILE_DESCRIPTOR_SET: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/angzarr.descriptors"));

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
