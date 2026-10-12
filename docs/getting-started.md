# 安装与首次初始化

仅支持当前Linux AMD64 GNU/systemd方案，不迁移旧配置或旧发行布局。
先验证发行包SHA256SUMS，解压到独立临时目录；不要直接覆盖已有运行目录。
包根只有一个安装入口install.sh，支持setup与upgrade。安装脚本由root执行，服务始终用普通isarmg-xcos UID。

## 全新安装

复制包内 `config/xcos.json.example` 到私有临时配置，填写实际RTSPS公开域名、
共享媒体配置路径及按需修改bind/data_dir/独立录像盘。JSON中没有任何实例密钥。

```bash
chmod 700 /path/to/private-config
chmod 600 /path/to/private-config/config.json
read -rsp 'Initial administrator password: ' password; echo
printf '%s\n' "$password" | sudo ./install.sh setup /path/to/private-config/config.json admin
unset password
```

setup安装程序、专用UID、私有JSON和systemd unit，执行纯配置校验及一次init。
init自动创建data_dir/secrets.json（0600）、当前数据库、日志及录像目录；拒绝已有数据，不覆盖密钥。
配置目录为 `/etc/isarmg/xcos`（isarmg-xcos所有，0700），应用配置为config.json（0600）。
独立录像盘应预先挂载；安装器创建新的录像目录并生成两个unit的data_dir/录像盘写入权限。已有录像目录应授予服务UID所需权限。

安装RTSPS证书与私钥到私有配置目录，编辑mediamtx.env中的真实TLS/ICE参数。然后：

```sh
sudo systemctl enable --now xcos.service
sudo -u isarmg-xcos /opt/isarmg/xcos/bin/xcos status
```

配置网关和媒体端口见[配置说明](configuration.md)。验证HTTPS登录、RTSPS发布、WebRTC/HLS及录像增长。

## 升级

备份data_dir（含secrets.json）和私有配置，从已校验的新发行包目录执行：

```sh
sudo ./install.sh upgrade
```

upgrade记录原运行状态、停止服务、安装当前程序和unit、执行离线check；仅原先active才重新启动；不运行init、不替换私有配置或实例密钥。
若检查失败，保持停服并修复问题；不要通过删密钥或重新初始化绕过错误。
