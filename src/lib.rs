//! zcode-axi library target: exposes the modules so integration tests can
//! exercise pure logic and the protocol client without the live runtime.

pub mod classify;
pub mod cli;
pub mod commands;
pub mod error;
pub mod framediff;
pub mod notify;
pub mod ocr;
pub mod output;
pub mod proto;
pub mod runtime;
pub mod store;
pub mod tasks;
pub mod watch;
pub mod window;
