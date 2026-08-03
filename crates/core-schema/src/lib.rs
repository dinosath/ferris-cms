//! core-schema — schema model + JSON + structural validation + diff
//! (design Part II §2, Part IV).

mod diff;
mod model;
mod validation;

pub use diff::*;
pub use model::*;
pub use validation::*;
