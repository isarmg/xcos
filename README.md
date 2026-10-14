# xcos

自托管摄像头监控服务。Rust 控制面管理管理员、实例、摄像头、媒体操作与录像索引；固定版本的 MediaMTX 处理媒体接入与播放，xcoc 客户端保存摄像头凭据。

服务端仅支持 Linux AMD64 GNU。server 只运行，入口为 `bin/xcos --root ROOT`，没有初始化、安装、升级、卸载或离线维护子命令。它不解析代理来源和访客 IP，不配置或探测 Cloudflare、Caddy。

## 安装与运行

安装与维护来自独立 [xssi](https://github.com/isarmg/xssi) 仓库，以源码中 `config/xssi.lock` 固定的完整提交打包。下载当前发行包，在独立目录核验 SHA256SUMS 后解压；安装工具校验包内完整 SHA-256 文件清单。

```sh
systemd-ask-password 'Initial administrator password:' | sudo ./install.sh \
  --package /absolute/verified-package --root /opt/isarmg/xcos \
  --config /absolute/private-config.json --username admin
sudo systemctl enable --now xcos.service
curl --fail http://127.0.0.1:18106/readyz
```

安装脚本负责账号、目录、权限和两个 systemd 服务；独立辅助程序负责初始化数据库、管理员、密钥和媒体配置。首次安装保持停服。程序、配置、日志和临时文件集中在 ROOT，录像和实例业务数据可明确外置，systemd 文件使用系统目录。

升级从已安装 ROOT 执行 `upgrade.sh --root ROOT --version VERSION --xssc /absolute/xssc`。脚本验证下载包后，由 XSSC 自动停止全部写入服务、离线检查、备份、执行明确支持的数据步骤、切换、启动及验证真实就绪。未知旧结构和旧发行布局直接拒绝，不提供兼容分支。卸载由用户处理。

## 构建

准备 Rust 1.99.0、Node.js 26.7.0、Python 3.11+、OpenSSL 和 GNU 工具。正式打包要求干净源码、同版本 annotated tag 精确指向 HEAD、已发布的固定 xssi 提交。

```sh
output="$(mktemp -d /tmp/xcos-release.XXXXXXXX)"
bash scripts/package-release.sh --output-dir "$output"
```

输出 `xcos-VERSION-linux-x86_64.tar.gz` 和 `SHA256SUMS`；包内SHA-256 文件清单覆盖程序、辅助工具、资源及 systemd 模板。[详细文档](docs/README.md)。
