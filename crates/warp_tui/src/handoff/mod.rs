//! TUI local-to-cloud handoff state, presentation, and session integration.

mod block;
mod model;

#[cfg(test)]
pub(crate) use block::TuiHandoffBlockEvent;
pub(crate) use block::{TuiHandoffBlock, init};
#[cfg(test)]
pub(crate) use model::{TuiHandoffModel, TuiHandoffModelEvent};
