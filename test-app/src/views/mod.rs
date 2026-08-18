//! Page components for the test app.

mod fcm;
mod init;
mod jwt;
mod recorder;

pub use fcm::Fcm;
pub use init::Init;
pub use jwt::Jwt;
pub use recorder::Recorder;

/// Shared page container style.
pub const PAGE_STYLE: &str = "padding: 24px; max-width: 560px; margin: 0 auto;";

/// Shared primary button style.
pub const BTN: &str = "background: #3b82f6; color: #ffffff; border: none; border-radius: 10px; padding: 10px 14px; font-weight: 600; cursor: pointer;";

/// Shared card style for status panels.
pub const CARD: &str = "background: rgba(30, 41, 59, 0.7); border: 1px solid rgba(255, 255, 255, 0.08); border-radius: 12px; padding: 14px; margin: 10px 0; font-size: 13px;";
