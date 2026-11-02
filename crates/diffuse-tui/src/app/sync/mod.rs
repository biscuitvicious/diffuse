//! Flat synchronization — streaming append and reload with cursor preservation.

pub mod append;
pub mod reload;
pub mod scroll;
pub mod snapshot;

#[cfg(test)]
mod tests;
