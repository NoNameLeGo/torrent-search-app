//! # bt-core
//!
//! BT 聚合搜索的领域核心：类型、归一化、公共 HTTP 层。
//!
//! 这是 Rust 重写（`feat/rust` 分支）的第一个 crate。内容从
//! `src/lib/normalize.js` 逐条移植，目标是**语义等价**：同样的输入给同样的输出。
//! `test/normalize.test.js` 的用例原样搬到 `tests/normalize.rs` 做对照。
//!
//! 注意：模块名 `normalize` 与函数 `normalize` 同名，所以函数**不在 crate 根**
//! 重新导出（避免路径歧义），调用方用 `bt_core::normalize::normalize(...)`。

pub mod dom;
pub mod http;
pub mod normalize;

pub use normalize::{NumOrText, RawResult, TorrentResult};
