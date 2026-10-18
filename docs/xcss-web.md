# xcss-web 管理前端维护

当前入口为 `web/xcss-web`，仅通过 `xcss-web` 路径 crate 接入；主分支使用 Rust 生产构建。构建与验收命令见 [Rust 原生 Web 构建与测试](rust-native-web.md)。本文保留首次迁移时的流程和测量记录，旧 Node/Vite 命令与体积数字不代表本次改名后的验证结果。

管理界面的新实现位于 `web/xcss-web/`，使用 Rust、xcss-web-core 0.8.19 和浏览器 WASM；服务端保留 Axum，并兼容既有 HTTP API、管理员 Cookie/CSRF、客户端配对、媒体授权和数据库合同；可按 WASM 加载、缓存与响应需要配合优化。它采用 CSR，不会将摄像头解码或媒体转发搬入 Axum。

当前 `main` 已合并 xcss-web CSR 与 Rust 原生构建；直接在主分支构建和验收。下面的首次迁移流程与数字作为历史记录，正式发行和线上部署继续使用产品发布流程。

默认入口 `web/src/main.js` 加载编译后的 Rust/WASM；React 和旧 TypeScript 对照实现已移除，当前回归直接验证实际 WASM。

## 构建与验证

生产构建需要 Rust 1.99.0 的 WASM 标准库、Linux 构建工具及与锁文件一致的 wasm-bindgen CLI；浏览器验收另需 Node.js 26.7.0 和锁定开发依赖：

```sh
rustup toolchain install 1.99.0 --profile minimal
rustup target add wasm32-unknown-unknown --toolchain 1.99.0
cargo +1.99.0 install wasm-bindgen-cli --version 0.2.129 --locked
cargo +1.99.0 run --locked --manifest-path tools/web-build/Cargo.toml -- check --product-root .
cargo +1.99.0 run --locked --manifest-path tools/web-build/Cargo.toml -- test --product-root .
cargo +1.99.0 run --locked --manifest-path tools/web-build/Cargo.toml -- build --product-root .
npm ci --prefix web
npm run test:browser --prefix web
```

`tools/web-build` Rust CLI 使用独立的前端 Cargo workspace 与 `web/xcss-web/Cargo.lock`；退役的 Python 构建器及其专属测试已移除。构建先在临时目录生成 wasm-bindgen 输出，验证源码在编译期间没有变化，再更新 `web/pkg/`；Rust 资源构建器将 WASM、样式和字体纳入静态资源树。正式构建默认使用 release，开发构建可显式传入 `--debug`。`CARGO_TARGET_DIR`、`WASM_BINDGEN` 可指向独立缓存和已安装的指定版本工具。

发行资源 `assets/xcss-web-build.json` 记录浏览器源码、Cargo.lock、共享源码、包校验值及生成文件 SHA-256。字体许可、xssi 前端清单和服务端发行摘要仍由原有发布流程处理。`web/pkg/` 为可重新生成的输出，不应提交。前端是独立 Cargo workspace；CI 另外执行它的 `cargo fmt` 和 `cargo clippy -D warnings`，服务端 workspace 的 `--all` 不会覆盖此目录。

资源预算区分 WASM、原有完整字体和普通资源：WASM 不超过 4 MiB，单个字体保留原有 16 MiB 上限，其余单资源不超过 512 KiB，总资源不超过 16 MiB，并拒绝 source map。一次 release 构建的 WASM 约 1.37 MB（Vite 报告 gzip 约 384 KB）；这是构建体积，不代表运行速度提升。

## 代码与浏览器边界

- `src/lib.rs`：实例列表、配对信息、名称草稿、认证外壳、快照合并队列和事件流。
- `src/model.rs`：完整响应结构、枚举与空值合同、显示标签、名称限制及审计字段隐藏。
- `src/logs.rs`：UTC 日期草稿校验、回车应用、三个独立游标与历史页、人工媒体操作核对。
- `src/recordings.rs`：录像查询身份、失效结果清理、主动点击重建播放器与快照刷新保留播放器。
- `src/controls.rs`：串行 PTZ 命令、按住重复移动、松开/失焦/隐藏/关闭后停止及设备回执查询。
- `src/media.rs`：Rust WHEP、ICE/SDP、同源资源地址、取消与资源删除、重连及 HLS 回退状态。

JavaScript 只负责 WASM/CSS 启动、共享轮廓绘制，以及按需加载的 `hls.js` 浏览器媒体引擎。HLS 的票据、重试、状态及生命周期由 Rust 控制；绑定会拒绝异源和含用户名/密码的媒体地址。WHEP 失败后申请新的 HLS 票据，不复用已尝试的凭据。

摄像头观测有效期仍锚定请求开始时的 `performance.now()`，不会被浏览器时区、错误系统时间或之后的时钟跳变延长。视频每轮最多三次连接，等待间隔为 0、1、3 秒；失败后等待 15 秒再开始下一轮。离开页面会取消定时器、销毁 HLS、关闭 WebRTC，并删除已创建的同源 WHEP 会话。

## 验收与历史基线

退役的 TypeScript 对照实现、Hook 模拟器和 Vite 转换依赖已删除；`test:unit` 只运行当前 Rust 单元测试。真实 WASM 浏览器验收保留管理页面、录像/日志与日期合同，并直接验证 WHEP 资源恰好一次清理、迟到请求不覆盖替换流、观测自然到期、快照合并、云台停止串行、状态重试仅执行 GET、名称保存/刷新失败与较新草稿隔离。Node/Playwright 只承担浏览器验证；原始对照可通过 Git 历史查阅。

`tests/xcss-web-media.mjs` 额外检查实际 WASM 加载，并在 Chromium/Firefox 验证 WHEP、接收流方向、ICE 服务器、会话删除、HLS 新票据及异源凭据边界。此信令探针模拟 `RTCPeerConnection` 和播放 Promise，以确定性核对真实 Rust 状态机；本次未连接生产摄像头或生产 MediaMTX。`tests/xcss-web-startup.mjs` 验证默认入口、系统主题变更、手动主题优先，以及存储被拒绝时显式英文登录。CI 的所有 Web 构建任务都安装固定 WASM target 和 CLI；正式发行仍受原有 annotated tag、源码清洁度、完整清单和已安装生命周期验证约束。本次源码迁移不会自动发布或替换运行中的服务。

Axum 对内嵌 WASM 使用构建时生成的 gzip 变体；请求需接受 gzip 才返回该变体，并保留 `application/wasm`、`Vary: Accept-Encoding`、变体独立 ETag、HEAD 和 304。缺少变体时回退原始资源；开发目录保持直接读盘语义。HTML 提前 preload 同源 WASM。当前实际构建的 WASM 为 1,372,699 字节，预 gzip 为 378,999 字节；约减少 72.4% 传输字节，不代表页面或媒体播放速度的实测提升。通用协商单测及实际 Axum Router 的变体/条件请求回归均通过。

依赖审计覆盖后端与浏览器两份 Cargo.lock，均保持原有 RustSec 和 yanked 拒绝策略。发布包保留现有 SBOM，浏览器锁定包、来源和校验和另作为 `assets/xcss-web-build.json` 的 `cargo_packages` 补充清单随静态资源内嵌。

最终服务端 workspace 的 `cargo fmt --all -- --check` 与 `cargo clippy --locked --all-targets --all-features -- -D warnings` 均通过；跨层协议守卫实际读取生产 Rust 浏览器与随产品提供的请求实现，版本前缀及 MediaMTX 合同检查通过。
