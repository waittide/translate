//! 领域模型：翻译工程的数据结构定义与 `.trans` 工程文件的加载/保存。
//! 数据层依赖 `serde`；I/O 集中在 [`persist`]，其余类型保持纯数据。

pub mod language;
pub mod llm;
pub mod persist;
pub mod project;
