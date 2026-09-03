//! zcode-axi library target: exposes the modules so integration tests can
//! exercise pure logic and the protocol client without the live runtime.

pub mod cli;
pub mod commands;
pub mod error;
pub mod output;
pub mod proto;
pub mod runtime;
pub mod store;
pub mod window;
