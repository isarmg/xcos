# xcos

## 项目简要介绍

自托管的摄像头监控服务。Rust 控制面管理设备和录像，配套 MediaMTX 处理媒体接入与播放，摄像头凭据由独立的 xcoc 客户端保存。

## 项目功能

- 管理管理员、客户端实例与摄像头，查看设备和媒体状态
- 浏览器实时播放、云台控制、主/子码流及录像索引
- 短期媒体发布授权、服务端录像与媒体状态协调

## 适用平台

服务端仅支持 Linux AMD64 GNU（`x86_64-unknown-linux-gnu`）。生产环境需要 HTTPS 反向代理，以及客户端可访问、证书受信的 RTSPS 入口。

## 如何快速部署

解压发行包，将 `config/xcos.json.example` 复制为自己的 JSON 配置，核对监听地址、数据目录和录像目录。

```sh
systemd-ask-password 'Initial administrator password:' | sudo ./install.sh setup /absolute/config.json admin
sudo systemctl enable --now xcos.service
curl --fail http://127.0.0.1:18106/readyz
```

安装器创建普通服务用户、初始化管理员和内部密钥，并安装 Xcos 与 MediaMTX 的 systemd 服务。
启动前配置 MediaMTX 的 RTSPS 证书及客户端可达的 ICE 地址；Web HTTPS 由现有反向代理提供。
后续升级在新发行包中执行 `sudo ./install.sh upgrade`。

## 如何编译部署

在 Linux AMD64 上准备 Rust 1.99.0、Node.js 26.7.0、C 编译工具、Python 3.11+、curl、OpenSSL 和 GNU 工具。正式打包要求干净源码，且同版本 annotated tag 精确指向 HEAD；不能把未发布源码当成已有标签的制品。

```sh
rustup target add --toolchain 1.99.0 x86_64-unknown-linux-gnu
output="$(mktemp -d /var/tmp/xcos-release.XXXXXXXX)"
bash scripts/package-release.sh "$output"
```

脚本下载并校验固定的 MediaMTX，构建内嵌 Web 的服务端，输出归档与校验文件。随后按上面的全新安装步骤部署生成的包。

[详细文档](docs/README.md)
