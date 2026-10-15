fn main() {
    // 把 ui/app.slint 编译成 Rust（生成 `AppWindow` 等类型，由 src/main.rs 的
    // `slint::include_modules!()` 引进来）。
    //
    // ⚠️ 这里的 `.expect()` 是**构建期**失败，不是运行期 —— 与“永不 panic”的契约不冲突：
    // 它发生在 `cargo build` 里，UI 文件写坏时构建直接红，不会带到用户手上。
    // 2026-10-07 那版 spike 就是在这里挂过一次：`Cannot convert string to styled-text`
    // （富文本属性写成了 `string` 而不是 `styled-text`）。
    slint_build::compile("ui/app.slint").expect("slint build failed");
}
