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

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
