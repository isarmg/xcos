# 开发与验证

使用 Rust 1.99.0、Node.js 26.7.0 和 Cargo/npm 锁图，仅构建 Linux AMD64 GNU。先编译 Web 再编译 server：

```sh
npm ci --prefix web
web/node_modules/.bin/xcss-build-server --config "$PWD/xcss-web-build.json" --mode development --web-only --no-install
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
./scripts/check-deployment.sh
npm run test:unit --prefix web
npm run test:browser --prefix web
```

server 只有 `--root ROOT` 运行入口。配置在 ROOT/config/config.json；独立 xssi 辅助程序执行显式初始化和离线检查。tests/deployment.rs 验证真实离线适配器、默认嵌套录像目录、重复初始化拒绝、密码重置及密钥保持。部署脚本只检查当前固定目录和 runtime-only systemd 合同，不操作真实服务。

开发程序默认使用本次内嵌 Web。JSON development=true 且未绑定源码时，可显式设置 XCOS_DEV_WEB_DIR 为绝对开发资源目录；生产源码绑定程序拒绝目录覆盖。单资源预算、MIME、全清单和许可证由公共 Web 构建器验证。

正式包构建入口 scripts/package-release.sh，要求干净源码、annotated tag、固定 xssi 提交和独立 Ed25519 私钥。构建使用逐文件验证的 Git 源码快照；同一次编译导出真实身份、状态合同和 Web 清单，不从 server 维护 CLI 查询。签名包包含固定 MediaMTX 程序、合同、模板和许可证；安装/升级/维护回归由独立 xssi/XSSC 工具测试覆盖。具体使用见[项目首页](../README.md)。
